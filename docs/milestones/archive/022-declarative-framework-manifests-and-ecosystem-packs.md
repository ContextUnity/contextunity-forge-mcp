---
id: m-declarative-framework-manifests-and-ecosystem-packs
title: Declarative framework manifests and ecosystem packs
doc_type: contract
status: completed
depends_on:
- m-language-parsers-and-declarative-framework-packs
owners:
- src/engine/ast/
- src/engine/languages/
- src/engine/linker/
- src/core/semantic.rs
- src/core/typed_facts.rs
- src/core/models.rs
- tests/languages/
- tests/incremental/
- tests/mcp/
- docs/architecture/
- docs/adr/
- src/AGENTS.md
- benchmarks/cold_test_coverage_benchmark.py
- docs/reference/
- docs/runbooks/
- skills/contextunity-forge/
invariants:
- 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
- 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
- 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
- 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
- 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
- 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
- 'INV-FRAMEWORK-COVERAGE: Verify real Commerce and Library controls separately against frozen source, configuration, binary and database baseline identities. Report complete R/A/U/E counts and same-cohort per-language and framework comparisons; external classification is not exact resolution. Prove exact-edge gains without losing correct baseline exact edges, preserve intentional unresolved/ambiguous outcomes and negative precision controls, and disclose all changes, configuration effects and residual limitations. No percentage threshold or synthetic experiment is required. Timing, memory and throughput are not gates.'
- 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
started_at: 2026-10-18T03:37:52+00:00
handoff:
  completed_at: 2026-10-22T00:24:23.420296086+00:00
  duration: 92h 46m
  commit: d4a27b805b5e5420e45967ffa14c0631c6ad35c9
  target_base: 1924fa0cd934fa1413aa30bfcf7e22833ee0ebc1
  integration_evidence:
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: m-declarative-framework-manifests-and-ecosystem-packs
      authority_digest: 07afc2d3c9dcaf056b92ae442fa49f63dfcbcf1a9822f7a3eb0247ec421aeef3
      implementation_digest: a12826ec096c1885db3f607b1a775ac26420856536f223dc737250c08283334a
      target_base_digest: 32b0b7407f61f0379a14c68c94ebf7a7530db9e94b17123e9d7e6bf63b251c8d
      builders:
      - codex-sol-authoring-guide-builder
      - codex-sol-builder
      - codex-sol-final-scope-builder
      - codex-sol-htmx-builder
      - codex-sol-real-data-builder
      - codex-sol-real-residual-builder
      - m022-html-vue-sol-builder
    target_base: 1924fa0cd934fa1413aa30bfcf7e22833ee0ebc1
    review:
      worker_id: codex-sol-m022-integration-reviewer
      decision: pass
      summary: 'Independent final combined review of d4a27b805b5e5420e45967ffa14c0631c6ad35c9 against1924fa0cd934fa1413aa30bfcf7e22833ee0ebc1: complete99-path source/docs review reused by exact hashes;414 nonmanaged files unchanged; verification-only task and receipt preserve literal eleven-task prefix and all accepted history. Twelve completed task proofs, reviewed handoff-authority repair, explicit40-path scope verification, final docs scan0edits and current successful context binding inspected. Required final command passed strictClippy and758 all-target tests/0failed/1ignored; separate commitment16 not double-counted. Real-control SQL005/precision residuals preserved. No unresolved scope or contract finding. Exact report and evidence binding are retained in .forge/evidence/m022-handoff-final/integration-review-final.'
      scope_findings: []
      contract_findings: []
    verification:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 758
      tests_failed: 0
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 758
    tests_failed: 0
---

# Declarative framework manifests and ecosystem packs

## Outcome and purpose

Deliver bundled, typed framework rules for the supported cases below, and verify their behavior through production-seam tests and retained real Commerce and Library control evidence. Keep language grammar and lexical extraction in its owning language profile, put every framework convention in validated manifest data, and compose typed facts through the existing linker seam:

1. **Django & ORM Ecosystem (`django.toml`):**
   Django ORM receiver hierarchies (`ForeignKey`, `ManyToManyField`, `OneToOneField`, `Manager`, `QuerySet`), Class-Based View template and model bindings, Django REST Framework serializers and viewsets, and exact reverse URL references from `{% url %}` and `reverse()`.

2. **Starlette & FastAPI (`starlette.toml`, `fastapi.toml`):**
   Starlette `Route`, `Mount`, `WebSocketRoute`, `JSONResponse`, and `HTMLResponse`; FastAPI route decorators and `APIRouter(prefix=...)`; `Depends()` references, response models, and exception handlers.

3. **Nuxt 3, Vue 3 & Pinia (`nuxt.toml`, `vue-runtime.toml`):**
   Nuxt 3 file-based page routing and auto-imported `useFetch` / `useAsyncData`; Vue 3 Single-File Component `<script setup>`, `defineProps`, and `defineEmits`; Pinia `defineStore` state and action references.

4. **HTMX & Alpine.js Cross-Language Relations (`htmx.toml`, `alpinejs.toml`):**
   Cross-language linking from literal local `hx-*` request URLs to Django and FastAPI route endpoints, plus Alpine `$dispatch` to `x-on` listeners and `$store.<name>` references to declared stores. `hx-target` and `hx-swap` remain request context, not endpoint identities.

5. **GridViewSpec Pack (`gridviewspec.toml`):**
   Declarative rules for GridViewSpec tables, toolbars, filters, cards, charts, KPIs, tabs, and actions, with typed bindings to backend Python datasources and frontend Alpine/HTMX controllers.

6. **Framework Coverage Proof:**
    Verify Commerce and Library separately against retained, identity-bound real-control baselines. Report complete R/A/U/E counts, per-language and fixed framework-cohort comparisons, and exact edge gains and preservation. Preserve intentional unresolved and ambiguous outcomes and negative precision controls; disclose configuration effects, all changes, and residual limitations. External classification is not exact resolution. No percentage threshold or synthetic experiment is required by the current user-approved condition below.

7. **Comprehensive Documentation & Authoring Guides:**
   Reference documentation and step-by-step guides for the filename-derived manifest identity, typed TOML schema, supported package declarations and workspace overrides, authoring workflow, and exact-edge verification.

8. **Canonical Rust Navigation:**
    Resolve named Rust module-use forwarding, public re-exports and the local Cargo library identity to verified canonical targets, including the framework seam's writer and Workspace calls, with persisted cold/delta proof.

---

## Performance guardrails for every task

Read this section before Contract, Build, Review, and delivery for every task in
022, including Rust navigation and framework-pack data changes. These shared
guardrails preserve existing indexing and storage contracts; they do not admit
additional engine features or import unrelated changes from `main`.

Source snapshot: `main` at `2c06defd57c80503ad18bd73137a4f3ef084f9b0`,
[performance guidance](../architecture/performance.md),
[indexing architecture](../architecture/indexing.md),
[modularity](../architecture/modularity.md), and
[ADR 0012](../adr/0012-zero-regression-serialization-and-cold-build-latency.md).
The rules below retain that snapshot's requirements even when the current branch
does not contain its newer pipeline/cache owners. Record an inherited gap
explicitly; absence of an owner is not proof of conformance or authority to add
an unrelated subsystem. Preserve accepted historical receipts.

### Extraction and linking

> [!IMPORTANT]
> Invariant: **PERF-AST-TRAVERSAL.** Preserve the flat iterative `TreeCursor`
> enter/exit traversal and balanced scope events. Accumulate body bindings and
> value flow through `SinglePassExtractor`/`ScopeAnalysis` and finalize at scope
> exit. Reuse those summaries rather than rerunning whole-scope binding, body-flow,
> or final-return collectors for individual assignments or references. Keep the
> documented occurrence-local import/template-loader proofs and TypeScript
> returned-object checks bounded; they do not authorize another body-summary
> pass. Keep `QueryCursor` experiments outside default indexing and avoid
> per-node leaf-count FFI guards such as `named_child_count() == 0`.

> [!IMPORTANT]
> Invariant: **PERF-RULE-PREPARATION.** Load and validate manifests before parallel
> extraction, reuse effective `LanguageContext` state, and cache grammar-bound
> `RuleIR` candidates by current inputs. Select AST candidates by `kind_id`.
> Keep TOML/YAML parsing, manifest I/O, and rule compilation out of per-node and
> per-reference loops. Preserve warm thread-local parsers.

> [!IMPORTANT]
> Invariant: **PERF-LINKER-LOOKUPS.** Build language member and receiver indexes
> before reference linking and borrow them through existing lookup contracts.
> Reuse borrowed import/scope names and lookup scratch buffers. Keep JSON
> decoding, source/disk reads, SQL, and index reconstruction out of per-method-call
> resolution. Hydrate unchanged providers from canonical persisted typed facts;
> relinking performs no source-I/O recovery. Preserve bounded alias, reducer,
> candidate and dispatch limits and their explicit exhaustion outcomes.

### Memory, persistence, and cache identity

> [!IMPORTANT]
> Invariant: **PERF-LEAN-FACTS.** Keep complete analysis facts in typed facts and
> compressed `local_facts`, separately from lean navigation `nodes.details` and
> FTS projections. Keep workspace-wide full `TypedFacts` out of cold linking.
> In the SymbolCatalog pipeline, preserve `insert_typed(..., false)` stripping of
> `value_flow`, `bindings`, `rebindings`, and `param_types`; encode full storage
> payloads within extraction workers and release full facts immediately. Preserve
> public serialization, malformed-fact handling, and delta hydration semantics.

> [!IMPORTANT]
> Invariant: **PERF-BOUNDED-STAGING.** Preserve bounded producer queues and shared
> byte-budget backpressure, including exclusive admission of one oversized item
> and cancellation/join/cleanup on failure. Keep staged storage rows compressed
> with level-1 Zstandard. Persist and hydrate bounded chunks of at most 500 rows,
> also respecting SQLite's live parameter limit; release each payload chunk as
> it is consumed. Avoid a second workspace-wide map of encoded payloads or
> simultaneous raw-AST and persistence-row retention.

> [!IMPORTANT]
> Invariant: **PERF-ALLOCATOR-BOUNDARIES.** Preserve existing allocator trims at
> extraction/staging and persistence batch boundaries. Keep trimming out of
> per-node, per-reference, and per-MCP-request paths and avoid duplicate trims
> after completed builds or deltas. A lower RSS alone does not prove faster work.

> [!IMPORTANT]
> Invariant: **PERF-SQLITE-PUBLICATION.** Preserve isolated candidate ingestion
> with `journal_mode=MEMORY`, `synchronous=OFF`, `temp_store=MEMORY`, and
> `cache_spill=ON`, a memory-bounded page cache, and deferred secondary indexes.
> Before atomic publication, restore `journal_mode=WAL` and
> `synchronous=NORMAL`, checkpoint with `wal_checkpoint(TRUNCATE)`, and preserve
> the existing integrity/commitment validation and failure cleanup boundaries.

> [!IMPORTANT]
> Invariant: **PERF-STREAMED-STORAGE.** Batch high-cardinality writes, flush final
> buffers, stream canonical bytes into incremental hashers, and preload dictionary
> IDs for commitment paths. Preserve shared path/key ownership, lean dictionaries,
> and indexed reverse dependency queries rather than duplicate reverse tables.
> Avoid ad-hoc per-row/per-edge allocations, broad commitment joins, repeated
> source reads, and storing duplicate uncompressed source text in SQLite or FTS.

> [!IMPORTANT]
> Invariant: **PERF-DETERMINISTIC-SEALING.** Preserve bit-identical
> `forge-owner-sha256-v2` commitments across all 17 domains: 13 logical domains
> and 4 FTS domains. Keep canonical FTS token order `(term, docid, col, pos)` and
> `term_map`; memory hashing must agree with canonical SQLite scans. Keep staged
> publication, full/delta parity, and commitment-integrity proof intact. Preserve
> the measured separation of index creation and sealing rather than introducing
> their previously rejected contention-heavy overlap.

> [!IMPORTANT]
> Invariant: **PERF-CACHE-BINDING.** Bind extraction-cache identity to source
> SHA-256, normalized relative path, Tree-sitter grammar ID,
> `INDEX_SEMANTICS_VERSION`, and `effective_ruleset_hash`, including bundled
> framework data in the build fingerprint. Preserve canonical binary DTO field
> completeness, compatibility, and fail-closed cache misses. Keep manifest,
> grammar, and semantic changes from reusing stale extraction. Preserve the
> existing cache-hit path without adding JSON encode/decode or Zstd work to it.

### Bounded queries and evidence

> [!IMPORTANT]
> Invariant: **PERF-BOUNDED-QUERIES.** Keep symbol operations target-scoped and
> dependency traversals bounded. Use indexed declaration selection where complete;
> general AST patterns use bounded admitted-file scans with source-digest checks
> and explicit computation limits. Preserve depth-four test discovery, the
> top-50 graph-ranking window, FTS candidate/count horizons, deterministic paging,
> and existing query/output budgets. Preserve inventory-admission reuse and its
> root/adapter/database identity invalidation; avoid duplicate full scans on each
> request. Keep public responses free of internal timing fields.

> [!IMPORTANT]
> Invariant: **PERF-MEASURED-CLAIMS.** Separate extraction, linking, encoding,
> ingestion, index creation, sealing, and verification measurements and describe
> overlapping/nested phase totals accurately. Preserve corpus, exact command,
> source binding, configuration, host context, cold/warm state, and mutation
> sensitivity with every performance claim. Green semantic tests do not prove
> latency or RSS. Revisit rejected optimization hypotheses only with admitted,
> representative evidence. Obtain explicit user approval before benchmarks.

The 10-second Commerce aspiration, throughput/search-latency and density reference
values are guidance, not new 022 gates. The numerical performance budgets of
030/031 remain owned by those milestones. Existing structural bounds such as
the 500-row cap remain mandatory. Framework verification in 022 uses the real-control
semantic counts, exact identities, and precision condition below; timing, memory,
and throughput are not acceptance gates.

### Required disposition before acceptance

For each affected guardrail, record its owner and source proof or an applicable
runtime regression. Mark unchanged/out-of-scope mechanisms with a reason rather
than claiming an unmeasured performance pass. Resolve introduced violations in
their admitted owner before acceptance; retain inherited gaps explicitly.

Completed finding **PERF-022-01**: `RouteContext` collects lexical binding and
Python write summaries before observing buffered route candidates.
`receiver_binding` reuses those summaries instead of collecting a scope for each
assignment or prefix provider. The accepted generic-task proofs preserve lexical
shadowing and exact-target admission; [route extraction](../../src/engine/ast/routes.rs)
owns the implementation. Its wall-clock impact is not measured.

Coverage evidence: the accepted revision-5 [real-control SQL report](../../.forge/evidence/m022-final-framework-data-accepted-bdfcdbb/m022-final-framework-data-build/report.md)
binds retained Commerce and Library baselines and candidates by source inventory,
configuration, binary and database SHA-256, with complete R/A/U/E, per-language and
fixed framework-cohort comparisons. Keep exact rule attribution separate from
nonexclusive source-context cohorts; report absent or unmeasured cohorts explicitly.
E is not R. Verify gained exact identities and preservation of correct baseline
edges, retain intentional U/A and negative precision controls, and disclose changed
classifications, removed incorrect edges, activation effects and residual limitations.

---

## Tasks in this milestone

### task: typed-framework-semantic-linking

```yaml
task_ref: typed-framework-semantic-linking
target: Remove framework-specific behavior from extractors, validators, and linkers by expressing existing framework semantics through validated manifests and finite typed operations
proof_policy: seam-test-first
contract_revision: 13
scope:
- src/engine/languages/manifests/syntax.rs
- src/engine/languages/typescript/frameworks.rs
- src/engine/languages/typescript/value_flow.rs
- src/engine/languages/typescript/linker.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker/named_bindings.rs
- src/engine/linker/value_flow.rs
- src/engine/linker/resolution.rs
- src/engine/linker/contracts.rs
- src/engine/linker.rs
- src/db/delta.rs
- src/core/semantic.rs
- src/core/typed_facts.rs
- tests/languages/manifests.rs
- tests/languages/typescript/frameworks.rs
- src/engine/languages/manifests/nuxt.toml
- src/engine/languages/manifests/vue-runtime.toml
- docs/reference/framework-manifests.md
- docs/reference/framework-manifests-nuxt-vue-pinia.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/framework-packs.md
- skills/contextunity-forge/references/frameworks/declarative-rules.toml.example
- skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example
- src/core/commitments.rs
- src/core/models.rs
- src/engine/ast/mod.rs
- src/engine/ast/routes.rs
- src/engine/ast/visitor.rs
- src/engine/languages/dependency_registry.rs
- src/engine/languages/mod.rs
- src/engine/languages/python.rs
- src/engine/languages/python/fastmcp.rs
- src/engine/languages/python/projections.rs
- src/engine/languages/python/render_context.rs
- src/engine/languages/python/template_loaders.rs
- src/engine/linker/framework_relations.rs
- tests/languages/html/framework_relations.rs
- tests/languages/python/frameworks.rs
- tests/receivers/python_value_flow.rs
- tests/languages/typescript/dom.rs
- src/engine/languages/manifests/framework.rs
- src/engine/languages/manifests/django.toml
- src/engine/languages/manifests/web.toml
- src/engine/languages/python/value_flow.rs
- src/engine/languages/python/computed_receivers.rs
- src/engine/languages/python/receivers.rs
- src/engine/languages/typescript/mod.rs
- src/engine/linker/indexing.rs
- src/engine/languages/rust.rs
- src/engine/languages/rust/value_flow.rs
- src/engine/languages/vue.rs
- src/engine/linker/html_templates.rs
- docs/reference/languages.md
- docs/architecture/indexing.md
- skills/contextunity-forge/references/frameworks/django.toml.example
- docs/milestones/archive/022-declarative-framework-manifests-and-ecosystem-packs.md
- docs/milestones/022-declarative-framework-manifests-and-ecosystem-packs.md
- src/engine/languages/html/javascript.rs
- tests/languages/features.rs
- src/engine/pipeline.rs
invariants:
- 'Revision 13 preserves every revision 12 capability, restriction, accepted proof and historical receipt, including its actual imported-function typeof requirement and paused Q2/Q3 boundaries. It supersedes only the revision 12 ComponentConfigurationFact-only wire restriction to permit one default-false, omitted-when-false value_query boolean on the existing AnnotationReferenceFact, with its existing exact SourcePosition, existing annotation_references vector, strict typed/public/FlowStore codec, Invalid preservation and current bounds. Legacy absent means false. The existing TypeScript type_query traversal supplies typed value-namespace evidence for the same actual reference occurrence and owning Node; no additional AST pass, Reference kind, ReceiverHint variant, Node.details marker, fact vector, codec or general type-system capability is admitted. Prepare a sparse typed lookup once by actual owner, exact line/column and expression, with no declaration-line/column-zero or spelling-only join. Only this proven value query may retain an imported actual callable through ordinary canonical import/reference resolution; ordinary type identifiers, type-only imports and unsupported/incomplete evidence gain no function target. Existing runtime RED023 is retained truthfully as preimplementation proof of this unchanged typeof requirement, including ordinary relative imports; subsequent source/test changes are separately bound and it is not relabelled as a new candidate run. Prove legacy false, exact nested/source-position discrimination, actual imported function targets and ordinary type-namespace negatives within the two existing shared seams; all precision, mutation, real-control and performance obligations remain binding.'
- 'Revision 13 adds only src/engine/pipeline.rs to the official generic scope, making 60 paths, solely for the demonstrated existing delta input boundary in callable-import-delta-input-gap-025.md. For old/new changed callable configurations, borrow selected same-project immediate provider paths from validated descriptors and the existing inventory once per distinct config/rule/project, then read their indexed module export records by confined path/kind and combine changed extracted provider modules. Seed old/new public and local export names through existing configured_result_names/shared_owners selection before unchanged consumers are chosen; retain old configuration origin for removal, false and unsupported states, without granting callable authority from invalidation names. Reuse existing sets, manifest-change path and provider hydration attempted-set. No source/AST/full-fact reread, per-reference query, global by-name scan, reverse index, persisted config-name list, fabricated dependency edge, broad relink or delta-frontier redesign is admitted. The prior Rust task pipeline entry remains honestly its read-only production-seam witness history; this independent generic repair write admission changes neither Rust capability nor prerequisites. All other 59 paths and existing restrictions remain unchanged.'
- 'Revision 12 preserves every accepted capability, historical receipt and immutable proof through revision 11, including the current performance and audit repairs, and admits one additional scoped exported-callable import contour with implicit named-export and explicit declared root-alias access paths. It supersedes earlier revision-specific capability prohibitions only for this bounded real-control restoration. The approved packet .forge/evidence/m022-rebase-20261022/perf-repair/autoimport-repair-admission-021.md (SHA256 10d16dc70db1b09ec823f78a260d2e4e2dddaf5711315164f33e05770483bf27), Commerce diagnosis a393fcad273f969dfbda6674e144457423bc4d9593f591bdbcbd6717118082aa and complete Library loss ledger f4c6f9c615f02abb6c35b34ee2c99d59538eb78d8ae241a80912eb2753c6e36a bind the actual scope and proof. The two admitted future method-return/literal-factory mechanisms remain paused and their ready subtask rows unchanged; no delivery or full-target completion is inferred.'
- 'Add only a strict default-absent ComponentEnvironment.callable_imports descriptor with exports using existing ComponentExport directory/extension validation, aliases, options_key, enabled_key, scan_key and unsupported_keys. Bound selectors to 16, unique nonempty alias prefixes to 8, unsupported dotted key paths to 16 and 4 segments per path, and each key/alias to 128 UTF-8 control-free bytes; reject duplicate, unknown, malformed, traversal and overflowing data before linking. Initial nuxt.toml data selects immediate composables/.ts, aliases ~/ and @/, options imports, flags autoImport and scan, and unsupported srcDir/rootDir/extends/alias/future/imports.dirs/imports.imports/imports.presets overrides. Every framework spelling remains exclusively TOML data. No new root operation, framework branch, module graph, general alias engine, custom layer/source-root semantics, runtime evaluation, dependency, cache, AST pass or E placeholder is admitted.'
- 'Extend only the existing ComponentConfigurationFact vector with default-absent finite callable enabled/scan state, through the canonical typed/public/FlowStore codec and current limits; preserve legacy compatibility, raw Invalid evidence and complete classic publication. One proven literal config-callee/object and effective origin is required. Absent options/flags use true defaults; known false suppresses implicit lookup, while duplicate/dynamic/spread/malformed configuration and descriptor-declared unsupported override keys supply no callable authority. Existing components.enabled and configured-module membership remain independent. Explicit declared alias imports retain ordinary import semantics when implicit enabled/scan is false; unproved scope or alias-root overrides cannot borrow the default root.'
- 'Prepare one shared borrowed complete candidate catalogue per effective rule, nearest declared JavaScript package/project and proven config identity. Select only actual direct named local value exports in the configured immediate provider files, proving export.local to a same-file indexed callable Node, including a same-file exported alias only if existing export machinery already proves it. Type-only/default/re-export/missing/unsupported or incomplete competitors cannot manufacture exactness; same canonical Nodes deduplicate, distinct complete candidates stay ambiguous, and unknown alternatives remain nonexact. Preserve existing candidate8/alias64/reducer bounds and exhaustion. No basename/global-name search, unconfigured linked-workspace fallback or synthetic/factory-result target is admitted.'
- 'Implicit callable lookup follows existing lexical declaration, actual import, parameter, rebinding, source-position and write precedence. Explicit imports match a declared prefix, exact confined selected provider path and actual export name, then reuse the existing canonical import/call/reference seam. Imported names never bypass configured_unshadowed to borrow implicit authority; existing typeof references to imported functions retain their actual canonical function target without adding type-system semantics. Returned members, SDK result inference, unrelated imports/types/classes and the paused Q2/Q3 capabilities gain no new authority.'
- 'Reuse required_configuration_facts, existing compact provider/full-fact hydration and attempted-set once path, package exports, normalized import/reference handling, contracts, configured_result_names and shared-owner seeds. Include old/new exported names after provider deletion/rename/retarget and config/rule/activation changes so unchanged consumers relink; preserve reopened full-fact, edge multiplicity, coverage/evidence and equal-final cold/delta commitments. No additional reverse index, workspace full-fact clone, per-reference filesystem/SQL/JSON/TOML work or source-recovery path. Existing build.rs engine/core fingerprint invalidates semantics; no manual version bump or hash domain.'
- 'Extend only existing nuxt_autoimports_require_declared_project_and_preserve_local_shadow and finite_framework_operations_preserve_canonical_targets_and_delta shared seams, reusing existing negative phases and optionally the existing ignored-root/linked-manifest seam when needed. First compiled runtime RED101 uses legacy valid Nuxt package/config and real callable, explicit ~/ import and typeof target expectations, never future-field parse or compile failure. Prove arbitrary renamed rule/callee/directory/alias keys, local/import/parameter/write precedence, complete ambiguity/unknowns, missing-provider add, provider-only remove/rename/retarget/duplicate/restore with unchanged consumer, implicit disable/scan false with explicit import preservation, rule/package/config changes and reopened cold/delta parity. No test function/file or synthetic fixture tree. Keep forbidden unconfigured negatives and configured factory/external-result positives strict. All measured Commerce31 implicit pairs/32 occurrences and Library30 implicit pairs/31 occurrences plus 3 explicit alias calls and 1 typeof reference require individual actual-target restoration; preserve all97 accepted status/TARGET checks and disclose every complete-graph difference. One coordinator-owned shared execution/review lane; measurement020 raw exit1 is not relabelled RED101. Co-evolve the admitted schema/Nuxt reference and skill examples. All milestone performance guards and unresolved 10.4s wall/39s user target remain binding.'
- 'Revision 11 preserves all accepted capabilities and immutable proof history through revision 10 and admits only two additional finite mechanisms: Python hierarchy-member return policies and JavaScript/TypeScript literal-argument factory receivers under their current Cargo features. These additions supersede historical revision-only prohibitions solely for these mechanisms and their mechanical compatibility changes. No dynamic or safe-literal URL feature, general type graph, runtime evaluation, new language activation, new root operation, new linker, framework spelling branch, new dependency or independent cache is admitted. Framework API, method, tag and property spellings remain exclusively manifest data. The reviewed implementation packet .forge/evidence/m022-rebase-20261022/perf-repair/next-type-flow-contract-016.md (SHA256 0b1d693dc80991f8ef5d7ce1712faada31359cfb99e2f3a549139848451772ae), independently reviewed PASS in next-type-flow-contract-review-016.md (SHA256 c5e0c711f40b419af1ee682263b9a7fb2e2ac09264cd1e6a846b7657f856ae03), supplies the exact schema, owners and proof dimensions incorporated below.'
- 'Revision 11 also admits the cumulative performance repair frozen as HEAD 94ecdf92c0a1ce1fb5fe2eede433539400951d08 plus complete tracked patch SHA256 f7df85684efb77fff5a45a35f5cbc00e1407f227b3cdf62fe37f81b6ec2d5a1a, including every one of the 18 source_files in .forge/evidence/m022-rebase-20261022/perf-repair/runtime-inputs-016/binding.json. Its immutable binary SHA256 is 1692659971347f1cf4af2ed5b85f34de3fe5d32c2009ddd859b5e41d6f24a3ca. Later task edits cannot be attributed to runtime016. Existing PERF-022-01 and all milestone performance guardrails remain binding; the 10.4s wall / 39s user-CPU target remains OPEN until separately measured by the coordinator. This scope admission permits no additional AST traversal, delta-frontier or writer redesign; new type-flow work does not alter performance-only owners beyond the sealed repair without demonstrated authority.'
- 'Q2 adds default-empty ClassHierarchyReceiverRule.returns mapping complete declared member paths to strict tagged ReturnPolicy SelfReceiver (kind=self) or Receiver { base } (kind=receiver); absent means no policy. Unknown fields/variants, self plus base, duplicate keys, undeclared member keys and malformed identities fail closed. Explicit base selects one effective hierarchy row in the same pack by its existing ordered bases identity; missing or multiple surviving targets never yield exact proof. At most 256 returns per row and 128 policy-bearing rows per effective path/language; nonempty control-free keys/base obey existing name_bytes (default4096). Legacy rows with no returns gain no new cardinality restriction. Existing project-first ordered-bases substitution and syntax-id replace/disable happen before target/ambiguity validation; project replacement is valid.'
- 'One owned TypeTarget::ConfiguredReceiver carries a finite tagged hierarchy-row or receiver_type-row authority, original receiver/import provenance and optional independently proved generic platform fallback. Hierarchy identity is effective workspace/scope plus pack plus ordered bases; nominal identity is scope plus pack plus syntax-rule id, each binding effective payload digest. Member/property lookup dereferences that finite rule before unrestricted External fallback; absent members stay Unknown except through the separately proven generic fallback. Preserve local class/import origin separately from result rule. No fake declaration, local call edge, copied Rust member catalogue or second result cache is admitted. Existing binding/computed-result storage materializes owned results.'
- 'Q2 retains the actual winning Python hierarchy member row through PythonReceivers and the existing indexing registry callback; name/provider resemblance is insufficient authority. Add default-None SemanticResolver::resolve_member_result(receiver,member,owner,at)->Option<TypeTarget> in the existing SemanticContext. The canonical call evaluator resolves receiver once and preserves local callable/annotation/C3/conditional/write precedence before applying external return policy. Self clones the proven original receiver including local subclasses/overrides; explicit policy switches finite authority while retaining origin. Compound Model objects.filter-style entries use the longest proven class prefix plus complete remaining member path through current lookup; they prove no intermediate Manager/Model instance and preserve local objects suppression. Initial Django data is only QuerySet all/filter/exclude self and Manager all/filter/exclude plus Model objects.all/objects.filter/objects.exclude explicitly returning the existing QuerySet rule. count/get/create/first and other members gain no policy.'
- 'Q3 adds optional default-absent omitted ValueExpr::Call.site: CallSiteEvidence { position: SourcePosition, receiver: Option<Box<ValueExpr>>, arguments: Vec<StaticArgument> }, with StaticArgument String { value } or Unknown. Position is actual inner call start; nested receiver accepts only existing bounded static member/constructor/call forms. Add typed ReceiverHint::TypedCallResult { value: ValueExpr, member: String }, requiring Call value; preserve legacy CallResult decoding. Existing TS value/direct-receiver paths share one grammar AST constructor; existing Python computed-receiver/value paths share canonical construction for Q2 without literal-factory activation. Assignment and direct reduction use the same evaluator, owner plus actual inner position identity, existing outer coverage occurrence and current depth8/alias64/candidate8/summary bounds. No callee-only/line-only join, JSON-string hint convention, linker source parsing or second AST walk. site=None supplies no Q3 literal discriminator and never suppresses an independently proved Q2 return policy.'
- 'Q3 retains at most four source-ordered positional slots (indices0–3), each literal at most256 UTF-8 bytes and total at most1024 bytes. Fifth argument, spread, interpolated/nonliteral/unsupported escape or malformed/over-limit evidence never establishes subtype. Missing/unknown slots remain absent/Unknown; no identifier evaluation, constant propagation or concatenation. Existing grammar string nodes/decoder own decoding, optional evidence allocates only when useful. Strict default-empty ReceiverTypeRule.argument_factories has one clause per receiver row: existing Api::GlobalMember callee, argument0–3 and at most32 unique literals of at most256 bytes; at most128 active clauses/4096 literal entries per effective path/language. Reject invalid fields, duplicates and over-limit configuration before linking. Prepare exact language/API/index/literal lookup once, deduplicate same finite target and mark distinct targets ambiguous without rule-order winners.'
- 'Q3 requires proven unshadowed configured global/member identity and current lexical/write/source-order/optional-chain guards; local/reassigned/imported lookalikes never borrow authority. Initial web.toml JS and TS receiver_type rows map document.createElement argument0 input to HTMLInputElement properties value=string and script to HTMLScriptElement properties src=string; these are properties, never callable methods. Select subtype before generic HTML*/SVG* normalization. Preserve an independently proved generic Element result for absent/dynamic/mismatched/over-limit/ambiguous mapping and generic members; subtype-only properties remain unproved on ambiguity. Configured APIs lacking an old generic result gain no fallback. Wrong subtype properties, invented members and local type-name collisions stay unproved.'
- 'Full effective returns/factory payload, deterministic map ordering, ordered bases, normalized literal sets and scope/pack/rule authority participate in existing configuration/pack/prepared-context identity. Preserve canonical compressed full facts/FlowStore and typed reference payload; no navigation details/FTS expansion, second codec, workspace facts copy, source reread or per-reference JSON/TOML work. Existing typed_facts strict codec preserves raw invalid payload, bounds, legacy no-site compatibility and round trips. Existing Contracts value_contract/details/resolution_digest retains every resolution-relevant discriminator and provider-only callable return change with unchanged consumers; existing typed/full-fact selection and delta hydration consume once. Existing build.rs semantics fingerprint/rebuild admission owns wire interpretation changes. No new hash domain or delta frontier. Rust/Vue/HTML/template/frameworks compatibility writes only supply defaults/exhaustive matches; they activate neither mechanism.'
- 'Extend existing functions only in tests/languages/python/frameworks.rs, tests/receivers/python_value_flow.rs, tests/languages/typescript/dom.rs and tests/languages/manifests.rs plus existing in-file codec/reducer seams. No test function/file, suite relocation, new binary or synthetic fixture tree. Prove assigned and direct Q2 explicit-to-self chains, local-subclass self preservation, absent policy, local objects/method/field override, missing initial receiver, conditional/unknown/ambiguous bases, annotations, unknown writes and cycles; retain external rule/import evidence and zero fake local edges. Prove assigned/direct Q3 input.value versus script.src (or downstream string member), wrong properties/invented calls, generic fallback for missing/dynamic/spread/over-limit slots, global shadow/reassignment and local collisions. Rename source and TOML import/base/member/result/global/tag/property identities together; policy/mapping removal, disable, explicit retarget and ambiguity change only authorized results. First prove missing chains/properties with legacy schema where possible; missing capabilities require compiled runtime assertion RED101, existing GREEN behavior retains accepted proof. Record all unexecuted phases after early RED.'
- 'Reopened real writer/reader SQLite seams compare canonical full facts, edge identity/multiplicity, coverage/evidence and commitments after equal-final cold/delta literal edits, import/provider retarget/removal, method/result policy changes, local override addition/removal, mapping/activation removal and provider-only return-evidence edits with an unchanged consumer. Existing codec cases prove legacy no-site and malformed/over-limit typed evidence. No stale subtype/self result survives. Bind final seam bytes and admitted production owners before Build; retain complete affected languages/linker target checks and strict Clippy under TESTS.md. Preserve exact real Commerce/Library control/cohort expectations and negative precision evidence; no verified real-corpus script.src occurrence, promised gain or benchmark success is asserted. Co-evolve admitted framework-manifests/languages/indexing references, skill and framework examples for fields, bounds, fallback/ambiguity, profiles and rebuild behavior; no ADR decision change is admitted.'
- 'Revision 10 preserves every accepted revision 4, 6, 7, 8 and 9 capability, invariant and immutable proof history; it supersedes only historical revision-specific write boundaries. Extend existing factory_members and ComponentEnvironment module descriptors for two separate contours: A uniquely indexed callable setup-return members and configured implicit named exports; B scoped external callable result members with external evidence and zero local edge. No new root operation kind, framework spelling branch, module graph, renderer bridge, synthetic Node, new persisted fact vector, cache mechanism, AST pass or source-I/O recovery is admitted.'
- The factory_members wire shape adds shape=options (default, preserving all legacy fields/behavior) or shape=setup_return and optional configured_module. Setup-return accepts one completed bounded nonasync/nongenerator callable argument and final literal return, rejecting mixed options fields. Global APIs require an explicit configured_module association matching one effective ComponentEnvironment module plus package/configuration and lexical guards; package presence alone is insufficient. ComponentModule adds default-empty exports descriptors with directory/extension and call_results mapping callee identifiers to finite callable-member identifiers. Existing components and its enabled switch remain independent of configured module membership. All schema fields are strict, bounded, serde-validated data with unrelated-spelling proof.
- A reuses existing FactoryResultFact owner/binding identity and completed return/definition/write summaries. Retain only independently proven callable members with actual indexed declaration targets and positions; a closed typed member target distinguishes same-name and alias declarations from unsupported RHS, without optional-name fallback. State without an indexed Node, unknown complex RHS, conflicting returns, duplicate/spread/computed keys, writes, nonunique exports or callback-local bindings stay unresolved/ambiguous. Resolve actual lexical owner and SourcePosition before implicit configured export selection; local/import/parameter/rebinding precedence remains unchanged. The 105 Library and 89 Commerce inventories are candidates, not an all-resolved threshold.
- B derives a closed in-memory ResultObject/ExternalCallable carrier from persisted Project(Call,member), completed binding scope/position and exact effective RuleOrigin, configured module/package/configuration scope. Prepare immutable shared descriptors once; no redundant call-start fact, magic Builtin string, global t rule or fake target Node. Project/alias/Scoped reducers and joins preserve complete provenance and uncertainty; ordinary local/imported indexed composables keep their real targets. Only proven configured external members become E with no local edge; all six ordinary parameter controls per control remain U. The 77 Library and 79 Commerce candidates require per-identity lexical adjudication, not blanket E.
- Use existing reference_keys, changed old/new export/factory/configured-callee name seeds, shared_owners and scope filtering for missing-provider addition, deletion, rename, duplicate and config-only module changes. Include old active names on removal. Extend the existing required-configuration/full-fact dispatcher and attempted-set once-path canonical hydration, selecting only scope-bound configured directory/extension candidates from the compact catalog and checking actual exports/facts. No parallel reverse index, full-workspace relink/map clone or per-reference source/FS/SQL/JSON/TOML work. Fact-only callable alias/target/config changes participate in existing canonical contracts and warm delta; unchanged consumers/providers replay without source reads.
- Extend only existing shared five-operation, Nuxt/Pinia and destructured-composable test functions using observed control snippets and existing negative phases; no new test function, file or synthetic corpus. Prove both contours separately, all legacy options facts, unrelated renames, lexical/import/parameter/write/ambiguity controls, missing-provider add and config-only module removal/restoration, components=false, rule and package changes, actual alias target IDs, reopened facts/coverage and equal-final cold-delta commitments. Preserve raw coverage identities, all prior ordinary/static grammar and precision outcomes. Record unexecuted phases honestly after early RED.
- 'All milestone performance guardrails remain binding: single iterative AST walk and completed summaries, bounded preprepared rule/provider descriptors and reducers, lean canonical typed facts, existing publication/commitments and source/grammar/effective-rule cache identity. Existing build.rs engine/core-source digest supplies extraction invalidation; prove it rather than invent another cache or constant. Runtime004 frozen controls are the actual baseline; final same-control per-identity R/A/U/E and target evidence separates A exact gains from B external classification, without baseline rerun, percentage promise or timing claim. Bare-fetch data remains existing-operation data only.'
- Revision 9 preserves every accepted revision 4, 6, 7 and 8 capability, invariant and immutable proof history. Current remediation is only generic static member admission for legal dollar identifiers and nested optional static property segments in existing JavaScript/TypeScript and embedded HTML/Vue callers, based on the frozen runtime002 real-control cohort. Reuse existing grammar helpers, Reference facts and implicit_builtins operation; no framework spelling branch, new DSL operation, metadata, schema, codec, ownership inference or cache mechanism.
- Preserve raw persisted expressions for newly admitted optional chains; canonical comparison may normalize only grammar-proven static segments. Do not remove coverage identities to manufacture improvement. Preserve existing single-optional normalization behavior or account its actual identity migration explicitly. Dynamic index keys, call-result chains and unproven receivers remain uncertain; retain lexical/import/write/reassignment/ambiguity guards and existing resolution precedence, with no early unconditional builtin admission or guessed exact edge.
- Reuse the existing single AST walk and bounded per-callee static-path parsing. No extra file pass, recursive body collector, repeated whole-source regex, module graph, per-reference filesystem/DB lookup or numerical performance promise. Existing full engine-source build fingerprint must invalidate changed reference extraction semantics; prove its binding, not a new cache system.
- 'Extend only the existing shared five-operation production matrix using observed control snippets and existing negative cases; no new test function or fixture tree. Real runtime002 provides the pre-fix witness: window.Alpine.$data153JS+3HTML and optional window.GridView.byId.get46JS+3HTML remain U while the normalized TypeScript row is E. Final same-control readback must preserve all identities, separate U-to-E from R gains, retain dynamic/call-result uncertainty and precision exceptions, and preserve reopened writer/cached-provider cold-delta commitments. No percentage threshold, setup-store, callable-return or cross-scope renderer feature is admitted.'
- Revision 8 preserves every accepted revision 4, 6 and 7 capability, invariant and proof history, including absent symbol_attachment slots emitting no facts. Its current remediation supersedes only the historical revision-specific write boundaries and admits F1 ordinary exact class/same-object field preservation and F2 containment of unrelated JS/TS property-reference emission. Restore prior ordinary grammar admission while additional consumers require association with existing effective configured FactoryResultFact providers, retaining import/autoimport, lexical, positional, write, uniqueness, missing/dynamic/ambiguity guards. Do not suppress all unknown property reads or numeric operands. Use existing candidate, factory-association and member/resolution owners; no new operation, framework spelling branch, schema, codec, cache, SDK inference or AST pass. HTML interpolated URL unresolved outcomes and real duplicate Python route ambiguity remain correct precision and must not be restored to exact edges. The existing five-operation shared matrix alone proves ordinary field edges, unconfigured reads and write-only LHS non-admission, configured local/imported/Vue state and action consumers, missing/duplicate/dynamic/shadowed controls, rule/provider/consumer transitions, unrelated spellings, reopened coverage and equal-final cold/delta commitments. No new test function is admitted.
- Revision 7 preserves all accepted revision 4 and revision 6 capabilities, invariants and proof history. Current remediation writes only the existing Python symbol_attachment projection to distinguish a proven absent configured target slot from present dynamic values or uncertain lookup. Proven absence emits no attachment fact or attachment coverage; present unknown or uncertain inputs remain unknown, explicit missing target names remain unresolved and duplicate target names remain ambiguous. No new operation, schema, codec, linker behavior, framework spelling switch or capability is admitted. The existing five-operation shared matrix retains unrelated renaming, source/target controls, rule transitions, ordinary edges and cold/delta/commitment proof.
- Revision 6 preserves every accepted revision 4 capability and prior proof history. Remediation changes only generic classified HtmlRequestAttributes URL admission, selection of unchanged route providers for those classified requests through the same existing required-full-typed-facts dispatcher, and existing factory-result resolution for Vue embedded JS-family owners; preserve lexical/import/write/unique/ambiguity guards and cached unchanged-provider cold/delta parity. No framework spelling tests, schema, cache, AST changes, new operation or feature is admitted.
- src/core/commitments.rs is an unchanged production proof dependency only, at SHA-256 e14ad8c8e257eb0a1f68ad109d9669e8e0668f54fe1a5e59ab909b8ee466ba7b; no writes to that file are admitted.
- Generic operations never contain framework spelling switches, special framework caches, or fixed framework attribute/path policy.
- Preserve existing observable framework behavior through effective manifest rules; preserve language grammar, exact-target admission, unresolved/ambiguous evidence, and delta source-I/O boundaries.
- 'PERF-022-01: Route observation reuses collected lexical binding summaries; processing individual assignments or prefix providers never triggers a fresh full-scope binding walk. Preserve lexical shadowing and exact-target admission.'
- 'AC4-1: The five finite operations and shared enums specified below are the entire new engine capability; all API spellings and activation are effective-rule data.'
- 'AC4-2: A real persisted seam proves response-model/exception symbol attachments, static page routes, options-factory state/actions, named store/event/renderer/action joins, and direct nested datasource URL+method joins, with exact canonical targets.'
- 'AC4-3: Unsupported, shadowed, missing, duplicate and conflicting identities emit no exact edge; persisted coverage distinguishes unresolved and ambiguous outcomes.'
- 'AC4-4: Rule removal/replace/deactivation, unrelated spellings and provider/consumer edits preserve ordinary language edges and equal-final cold/delta facts, coverage and commitments, without unchanged-provider source reads.'
- 'AC4-5: New facts use the existing strict typed envelope and canonical decode/validation; all current performance guardrails and the existing PERF-022-01 contract remain binding.'
- 'AC4-6: All four new vectors and factory-result association participate in the existing canonical resolution identity and typed required-full-facts dispatcher. Fact-only function-scope provider retargets, domain/method changes and unknown/conflict transitions relink unchanged consumers from cached facts, with cold-delta parity and no parallel hash, source recovery or full-workspace fact duplication.'
subtasks:
- subtask_ref: typed-semantic-operations-and-validation
  title: Add only the typed relation and activation forms required by the admitted framework packs, with fail-closed validation
  status: completed
  evidence: Implemented typed SemanticTarget::ImportedSymbol, SemanticRelation::InjectsDependency, the call_to_symbol wire representation, and fail-closed validation for the admitted operation shape. Verified by the full post-review command `cargo clippy --all-targets --all-features -- -D warnings && cargo test` (exit 0; 742 passed, 0 failed, 1 ignored) and the focused unit test typed_symbol_operation_is_decoded_and_validated.
- subtask_ref: compose-language-facts-through-existing-linker
  title: Compose facts emitted by existing language profiles through framework_relations and retain resolved, ambiguous, and unresolved coverage
  status: completed
  evidence: Composed exact imported-call facts through framework_relations, resolving only a unique local callable and retaining unresolved/ambiguous persisted coverage. Integration regression `framework_manifest_symbol_relation_links_exact_dependency_injection_target` verifies exact, duplicate, missing, inactive-package, assignment, augmented-assignment, deletion, loop-target, and formal-parameter-shadow cases through writer::build and persisted SQLite edge/coverage queries. The post-review full command `cargo clippy --all-targets --all-features -- -D warnings && cargo test` exited 0 (742 passed, 0 failed, 1 ignored).
- subtask_ref: finite-pack-projections-and-persisted-joins
  title: Implement only the five proposed generic operations through existing profile and LanguageLinker owners
  status: completed
  evidence: Implemented the five closed generic operations and four strict typed vectors through existing profiles, canonical codec/identity, borrowed named/factory index and ordinary language/request resolvers. Shared persisted matrix proves exact canonical targets and both renamed spellings. R1/R2 bounded repairs preserve ambiguous operands and closed file-route grammar. Final exact registered Clippy + complete languages passed266/0; linker102/0, incremental30/0; same reviewer CLI fixtures now pass.
- subtask_ref: projection-causality-precision-and-delta-proof
  title: Prove exact targets, fail-closed controls, effective-rule causality and unchanged-provider cold-delta parity through one real persisted seam
  status: completed
  evidence: 'One existing shared real manifest/writer/reopened-SQLite matrix passes original+renamed spellings, negative precision/ambiguity, compressed vectors, provenance, dependencies, activation/removal/replacement and provider/consumer cold-delta parity. Extended only symbol_attachment/file_route rows for R1 3x3 operands and R2 static segment boundaries. Focused1/0; complete languages266/0, linker102/0, incremental30/0; immutable unit owners retain codec3/0+identity1/0 proof. Logs: .forge/m022-generic-r4-repair-focused.log and repair-registered-command.log; consolidated closure packet follows.'
- subtask_ref: perf-022-01-reuse-collected-scope-bindings
  title: Eliminate repeated full-scope binding collection in generic route observation
  status: completed
  evidence: RouteContext completes canonical lexical binding/Python write summaries once in the shared prepass. All five receiver consumers and router uniqueness read completed summaries; assignment/prefix processing does not invoke fresh recursive scope collectors. Existing non-route compatibility collectors retained as admitted by PERF design. Final strictClippy0; full languages266/0, linker102/0, incremental30/0. Structural proof retained in .forge/m022-perf-sol-evidence.md and independent review packet; no benchmark or global collector-removal claim.
- subtask_ref: honor-classified-html-request-url-admission
  title: Honor existing generic HtmlRequestAttributes URL classification before exact route admission; prove static positives and dynamic/nonlocal fail-closed controls through the shared persisted matrix
  status: completed
  evidence: 'Revision6 Build32: composer honors existing URL statuses; shared effective-origin guard selects classified local literal unchanged route providers through existing typed dispatcher and RouteIndex. Shared matrix original+renamed dynamic-literal-dynamic, exact persisted targets, remote/current-page coverage and cold-delta parity passes1/0. Registered strict Clippy plus complete languages267/0 exit0. Logs .forge/evidence/m022-generic-r6-build-focused.log and m022-generic-r6-build-final.log.'
- subtask_ref: preserve-factory-result-resolution-for-vue-owners
  title: Resolve existing factory-result facts for Vue embedded JS-family owners through the shared state/action matrix while preserving lexical and exact-target guards
  status: completed
  evidence: 'Revision6 Build32: existing JS-family factory-result hook admits Vue owners while retaining lexical/import/write/position/unique/ambiguity guards. Same shared matrix verifies canonical template-scope state/action edges, lexical shadow negatives, provider and rule transitions and equal-final cold-delta parity for original+renamed spellings. Focused1/0 and registered strict Clippy plus full languages267/0 exit0. Exact template-source oracle correction recorded in Contract31 closure map; no new testfunctions.'
- subtask_ref: finite-method-return-policies
  title: Prove finite hierarchy method return policies through existing assigned/direct Python receiver and persisted delta seams
  status: ready
- subtask_ref: literal-argument-factory-receivers
  title: Prove bounded literal factory subtype properties and generic fallback through existing JS/TS receiver and persisted delta seams
  status: ready
- subtask_ref: scoped-exported-callable-import-restoration
  title: Restore actual scoped callable exports and declared root-alias imports through the existing shared Nuxt and cold-delta seams
  status: ready
```

#### Acceptance

1. Declarative rule variants cover the admitted template, route, model, dependency-injection, store, event, datasource, and controller relations using typed source facts and target identities.
2. Unknown fields, unsupported relation/language combinations, malformed identities, and invalid activation values fail closed. Manifest identity continues to come from the filename stem; no `[framework]` metadata table or arbitrary extraction code is introduced.
3. Relation composition stays in the existing `LanguageLinker` framework-relations seam. Emit an exact edge only for one verified target; preserve unresolved and ambiguous coverage without emitting an edge.
4. Seam tests cover package activation, unconditional rules with explicit extracted facts, local workspace scope, unique targets, absent targets, duplicate targets, and `extend` / `replace` behavior.
5. `cargo test --test languages` and strict Clippy pass.
6. Remove hardcoded framework behavior throughout the existing extractor paths, including shared router/controller prefixes, Python Django/DRF/template/MCP handling, and TypeScript/HTML framework conventions. Keep reusable grammar, binding, literal, and target-resolution helpers under their existing owners. Move semantic choices to typed manifest fields rather than renaming framework branches.
7. One persisted production-seam matrix proves unrelated pack/API/attribute/path names, rule removal and replacement, activation, shadowing, unique/missing/duplicate targets, static/dynamic/nested prefixes, and cold/delta equivalence. Red evidence uses the intact baseline; deleting working bundled rules never substitutes for a missing-capability proof.
8. Independent architecture review traces every remaining framework-specific choice to manifest data. Synchronize affected architecture, reference, source instructions, repository skill, and examples in the build.
9. Admit only the five finite operation forms below, through strict existing rule envelopes. Prove each new capability with an actual pre-implementation RED through the real writer, reopened SQLite and delta; preserve existing green behavior through direct proof rather than manufacturing RED. Test scenarios share the coherent production seam and existing fixtures.
10. Prove each new fact vector and factory association in canonical resolution identity, including function-body provider key/callback retargets with unchanged declarations, imports, ordinary callees, returns and fields. Keep cross-language consumers unchanged; cover missing-to-present, unknown/conflict-to-unique, domain and method changes, cached unchanged-provider replay and equal-final cold/delta commitments.
11. PERF-022-01 reuses collected lexical binding summaries through the existing shared extraction walk. Preserve shadowing/exact-target and cold/delta proof; do not add AST passes, per-reference decoding, benchmarks or numerical performance promises.

#### Revision 4 finite operation boundary

Use the existing `semantic_rules` and `syntax_rules` envelopes, filename identity, activation, provenance and relation machinery. New nested structures and enums reject unknown fields. Required operation fields have no silent defaults. All API, keyword, attribute, directory and named-domain spellings are manifest data.

| Operation | Required operation fields after `kind` | Admitted result |
| --- | --- | --- |
| Semantic `symbol_attachment` | `language="python"`, `callee:Api`, `source:AttachmentSource`, `target:Slot`, `relation="uses_model"\|"references"` | Decorated callable → class model, or decorated/registered handler → exception class; no exception-dispatch relation. `uses_model` requires decorated source; source/target argument slots differ. |
| Syntax `file_route` | `directory`, `extension`, `index`, `method="GET"`; outer language `vue` | Module-owned static route using existing route containment/index. Normalize relative directory/suffix and final index basename only; bracket/catch-all/dot/empty/traversal segments remain nonexact. |
| Syntax `factory_members` | `callee:Api::Imported`, `argument`, `state_member`, `actions_member`; outer JS/TS | A uniquely bound factory returned by the configured call exposes proven static state/action declaration IDs when that factory is called; reject state/action collisions. |
| Syntax `named_binding` | `domain`, `binding:NamedBindingForm` | Unique literal-key definition/reference joins use `references`; ordinary proven member calls retain `calls`. Same mechanism serves named stores/events/renderers/actions. |
| Semantic `endpoint_projection` | `language="python"`, `callee:Api::Imported`, `attachment:{callee:Api::Imported,keyword}`, `url_keyword`, `method_keyword`, `default_method` | One direct inner constructor at the configured outer-constructor keyword yields owner → URL+method route (`calls_endpoint`) → backend handler (`handles`). Default method applies only when absent; invalid/dynamic values remain unknown. |

`Api` is the strict enum `imported{module,export}`, `constructed_member{module,constructors,member}`, `global{name}`, `global_member{receiver,member}`. Python permits the first two only; symbol attachment also permits only those forms. Factory and endpoint rules require imported APIs. Global-member receivers contain one or two literal identifier segments. Prove an unshadowed root and either the direct receiver or exactly one unique unreassigned local alias from existing collected bindings, including `var gv=window.GridView`; no further alias chain, branch or dynamic-property proof.

`Slot` is `argument{index}`, `keyword{name}`, or one-level `keyword_member{name,member}`. Python uses keywords/arguments, JS uses arguments; symbol-attachment targets exclude keyword-member. `AttachmentSource` is `decorated_callable{}` or `argument{index}`. `NamedBindingForm` is `call_definition{callee,key,target,target_kind="object"|"callable"}`, `call_reference{callee,key}`, `member_reference{root}` (JS/TS), or `attribute_definition{prefix,scope}` (HTML). Definition key/target slots differ. Member references select the first static member after the configured proven root; remaining members use the canonical member index. Attribute definitions register the real embedded listener scope through existing embedded-JS extraction, shared with overlapping script-attribute rules.

Bounds: argument indexes below 32; distinct valid identifier slots; at most 32 constructor alternatives; nonempty API/domain identifiers at most 128 bytes; literal named keys are nonempty, control-free and at most 256 bytes; at most 128 static object members without duplicates. URLs are literal local slash-prefixed paths. Unsupported/spread/computed/duplicate inputs remain nonexact. A dynamic named definition cannot be discarded to manufacture uniqueness in its domain/project.

Add only four default-empty typed vectors in existing `ValueFlowFacts`: `symbol_attachments{origin,position,source,target,relation}`, `factory_results{origin,position,binding,members}`, `named_bindings{origin,position,domain,key,role}`, `endpoint_requests{origin,position,url,method}`. Use existing rule provenance, positions, object-member identities and `FlowStore` scope. Symbol operands are `node{id}|name{expression}|unknown{}`; static strings are `literal{value}|unknown{}`; named roles are `definition{target,target_kind="object"|"callable"|"listener_scope"}|reference`. Canonical decode/validation belongs to `core/semantic.rs` and existing `core/typed_facts.rs` codec: absent vectors decode empty; malformed/unknown/oversized payloads invalidate the containing flow. Limit each vector to 128 per scope with existing exhaustion outcomes; no old-binary forward-compatibility promise.

All vectors and factory association enter the same canonical `linker/contracts.rs` resolution projection for callable and raw scopes. Select required full typed facts before lossy public `Facts` conversion in `linker.rs`; retain the existing borrowed typed-flow handoff. Named cross-language discovery projects only compact domain/key selectors onto existing indexed `Reference` storage (`semantic_named_definition`/`semantic_named_reference`, real source ID, domain in module, key in expression, empty only for unknown). These selectors emit no ordinary graph reference and duplicate no target payload. Existing delta reverse queries use old/new selectors, missing-provider discovery and unknown-key domain/project invalidation, then decode selected cached providers once through existing convergence/hydration. Add no DB schema/hash, full-workspace fact clone, consumer codec, source recovery, parallel linker/cache, framework spelling branch or general query/VM language.

---


### task: rust-canonical-use-and-local-crate-bindings

```yaml
task_ref: rust-canonical-use-and-local-crate-bindings
target: Resolve Rust named use forwarding, public re-export chains, and the local Cargo library identity to canonical indexed declarations and existing associated-call targets
proof_policy: seam-test-first
contract_revision: 8
depends_on: []
scope:
- src/engine/languages/rust/linker.rs
- tests/languages/profiles.rs
invariants:
- 'Revision8 corrects writable ownership only: scope contains exactly src/engine/languages/rust/linker.rs and tests/languages/profiles.rs. Preserve every accepted capability, immutable HEAD94 prerequisite, historical receipt and revision7 production-seam witness proof. The historical unchanged pipeline.rs witness remains evidence, not Rust writable ownership; the independently admitted generic revision13 may repair its callable-configuration delta input without a dependency cycle or Rust behavior change. Existing preimplementation RED and shared verification evidence remain truthfully bound to their original snapshots; no new functionality, synthetic proof, policy change or source modification is admitted by this scope correction.'
- 'Admit exactly two capabilities: bounded use/re-export forwarding through existing import facts, and current-package library identity for integration-test imports. Preserve existing visibility and member-resolution guards without introducing a general visibility or module engine.'
- Only a unique statically verified, visible target produces an exact edge. Missing, ambiguous, cyclic, unsupported or exhausted bindings never use basename or cwd guesses.
- Collected Cargo identities and persisted typed use/export facts feed the existing namespace, alias and Rust member consumers; linking and delta never reread source or Cargo files to reconstruct binding semantics.
- src/db/writer.rs is an unchanged production-seam witness; this task does not modify writer behavior or source.
- Repair-only revision6 consumes the already accepted generic capability at immutable HEAD94ecdf92c0a1ce1fb5fe2eede433539400951d08 and its historical typed-framework-semantic-linking receipt. That accepted baseline is the prerequisite, not the reopened generic revision11; no paused method-return/literal-DOM mechanism or new generic API is required or implemented.
- 'Restore RUST-022-AUDIT-01 only: all bounded forwarding alternatives must be proven and collapse to one visible canonical target before exact admission. A surviving known target never hides a missing, inaccessible, cyclic, unsupported or exhausted alternative. Preserve existing visibility, alias-depth and fact limits and all accepted current-package library behavior.'
- 'Revision7 admits src/engine/pipeline.rs solely as the unchanged production composition witness reached by the existing persisted Rust test through writer::build. No writes to pipeline.rs are authorized; the only writable owners remain src/engine/languages/rust/linker.rs and tests/languages/profiles.rs. No new engine behavior or paused generic capability is admitted.'
subtasks:
- subtask_ref: rust-use-and-export-binding-facts
  title: Follow named parent use and public re-export chains through existing import facts and canonical lookup
  status: ready
  evidence: null
- subtask_ref: local-cargo-library-identity
  title: Collect bounded own-library identity and resolve it only to an admitted indexed provider
  status: ready
  evidence: null
- subtask_ref: persisted-rust-binding-proof
  title: Prove exact targets, fail-closed negatives, cold-delta parity and the live writer and Workspace bindings
  status: ready
  evidence: null
```

#### Acceptance

1. One persisted production-seam matrix in `tests/languages/profiles.rs` drives the real writer, Rust extraction/linking, reopened SQLite and delta. Observe a missing-capability RED before production changes. Cover named parent `use`, grouped and aliased `self` / `super` / `crate` paths, public function/type/module re-export chains, and existing inherent associated-call targets.
2. Collect the workspace-root package's own library identity through existing bounded Cargo manifest collection. Support the normalized package name and explicit `lib.name` for the conventional indexed `src/lib.rs` provider; retain disabled/absent-library and unrelated-package negatives. Custom library paths, nested-package own-library discovery, inherited workspace identities and unsupported Cargo forms remain unproven. Resolve the library from collected/indexed facts, without cwd guesses, dependency fetching or per-reference filesystem reads.
3. Preserve valid descendant access to ordinary private parent imports and existing Rust glob/member behavior. Reuse current visibility guards and associated-member resolution. Missing, inaccessible, duplicate, conflicting, cyclic, unsupported or exhausted bindings retain unresolved/ambiguous evidence without an exact edge. Keep the existing bounded alias policy and explicit exhaustion outcomes; add no general visibility, trait or macro-expansion engine.
4. Add/change/remove forwarding aliases, re-exports, providers and own-library identity through source or manifest deltas. Assert canonical targets, coverage and dependencies, cold/delta parity and valid commitments; unchanged providers are consumed from persisted facts. Named mutations reject dropped forwarding, wrong crate identity, first-candidate selection and stale export admission.
5. A stable index built by the repaired candidate resolves the actual `writer::build` and `Workspace::new` references in the framework seam to `src/db/writer.rs::build` and `tests/languages/support.rs::Workspace::new`. Record exact source/content binding, index generation, reference coverage and canonical destination IDs. Compilation and file-level seam admission alone do not satisfy this criterion.
6. Reuse existing extracted import/export facts, namespace indexes, LanguageLinker import hooks, SemanticContext and RustMembers. Settle the bounded forwarding path during contract work. Add no comprehensive binding DTO, shared AST visitor bridge, parallel linker/cache, opaque policy escape, task-engine workaround, DB schema or MCP protocol change.
7. Run strict Clippy, complete `languages` and `linker` targets, focused matrix and affected incremental verification under `TESTS.md`. Independently review correctness, ownership/visibility and proof on the same frozen candidate. Update the affected indexing/modularity descriptions with supported semantics and boundaries.
8. Exclude full logical module graphs, new `#[path]` handling, general `cfg` evaluation/provenance, custom `lib.path`, nested Cargo support, and a new method/restricted-visibility subsystem. These are not prerequisites for the two admitted capabilities. Preserve uncertainty where existing facts cannot prove a target; follow the shared performance guardrails without adding AST passes or per-reference decoding.

---

### task: django-and-orm-comprehensive-manifest

```yaml
task_ref: django-and-orm-comprehensive-manifest
target: Expand django.toml with ORM receiver hierarchies, Class-Based View template/model bindings, DRF serializers/viewsets, and reverse URL references
proof_policy: direct-proof
contract_revision: 5
depends_on:
- typed-framework-semantic-linking
scope:
- src/engine/languages/manifests/django.toml
- tests/languages/python/frameworks.rs
- tests/languages/html/framework_relations.rs
- docs/reference/framework-manifests.md
- src/db/writer.rs
invariants:
- This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
- src/db/writer.rs is admitted only as an unchanged production proof dependency at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a from 9631fe402b69a27bb01f7cec763928178a591d0f; engine writes remain forbidden under this Django task.
subtasks:
- subtask_ref: django-orm-model-and-field-receivers
  title: Add complete class_hierarchy_receivers for Django models, querysets, managers, and relations
  status: completed
  evidence: cargo test --test languages django_ (exit 0; 8 passed, 0 failed), including persisted external receiver coverage for QuerySet, Manager, ForeignKey, and unresolved unrelated receiver.
- subtask_ref: django-cbv-template-and-model-semantic-rules
  title: Link literal Class-Based View template_name and model attributes to unique templates and model declarations
  status: completed
  evidence: cargo test --test languages django_cbv_attributes_link_only_unique_templates_and_models (exit 0; 1 passed, 0 failed); persisted unique edges and missing/duplicate/dynamic coverage.
- subtask_ref: django-drf-serializer-and-viewset-rules
  title: Add tested receiver and route rules for Django REST Framework serializers and ViewSets
  status: completed
  evidence: cargo test --test languages django_ (exit 0; 8 passed, 0 failed), including persisted Serializer/ViewSet receiver coverage and static/dynamic/duplicate router registrations.
- subtask_ref: django-reverse-url-resolution
  title: Resolve literal {% url %} and reverse() references to named routes without fallback matches
  status: completed
  evidence: cargo test --test languages django_ (exit 0; 8 passed, 0 failed), including persisted template url tag, reverse import aliases, missing route, and duplicate route coverage.
status: completed
receipt:
  commit: 6ac6268656476d41f9aaf471ae106a4aceb13f69
  contract_revision: 5
  passed_at: 2026-10-21T01:20:06.315177767+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 266
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All six scope/managed-document hashes match Build; all five task owners match accepted snapshot8a455598. Exactly existing causal test and approved managed authority amendment differ from HEAD9631fe4. All169 src bytes unchanged; untracked evidence only.
        claims:
          applicable: true
          evidence: Acceptance1-4 inspected in Django TOML, ORM/DRF,CBV,named-route and supporting existing cases; six real rule/source states with persisted facts/coverage/canonical edges, ordinary calls/imports, commitments and cold/delta parity. Independent strictClippy0 and full languages266/0; no finding. Bound packet .forge/evidence/m022-django-review-packet.md SHA256 4018cf82d8c3e1fd840771104096c163dbc7965f973c12c380472f404c10efc1
        concurrency:
          applicable: true
          evidence: No concurrency/revision/transaction/publication code changes; existing owner bytes unchanged. One Cargo scheduler, terminal checks, no concurrent product writes. Fresh lifecycle claim/candidate fencing retained.
        project_isolation:
          applicable: true
          evidence: Explicit Django root; fixture-local workspace/coldSQLite, existing project/library isolation controls pass. No root/profile/path/namespace implementation changes or main/remote integration.
        administration:
          applicable: true
          evidence: Current contract5/digeste5033094/Review25 independent codex-sol-django-reviewer vs builder codex-sol-builder. Writer exact unchanged proofdependencySHA9a5684b13 preserved. No manual receipts/profile/commit edits; supported automatic delivery only. No unresolved/deferred Django finding.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: A fixed in-repository framework fixture measures resolved / (resolved + ambiguous + unresolved), excluding external imports. The candidate must exceed both the measured branch-point baseline and 85%; only coverage counts and percentage are acceptance evidence. Timing, memory, and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
    - src/db/writer.rs is admitted only as an unchanged production proof dependency at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a from 9631fe402b69a27bb01f7cec763928178a591d0f; engine writes remain forbidden under this Django task.
    architectural_notes:
    - 'decisions: User explicitly approved administrative proof-boundary amendment. ONLY Django contract revision4 to5, exact src/db/writer.rs scope addition, invariant admits it only unchanged at SHA9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a from9631fe4; engine writes remain forbidden. Supported sync revoked claim21 and returned ready22; fresh author claim23 digest e50330949b2632b1096218289f7fc7fefe30aed5de0811c6c3c4c3087da80a0a. Protected header, all siblings/receipts/dependencies/acceptance/subtasks byte-preserved. Full diff .forge/evidence/m022-django-manifest-r5.patch SHA8f6a85e9f597c7f100b8aeb2b4b2939ddb0ae3fe1f1b023936074a318d07c9b0; manifest before8a080587f9628ae5cfa0d4b3a5f9c35026245d4e1a0ffd75f4a76a5bbfa5a4c5 after6fbf471cc62789fc9f9613eaca80feb794ff35d597d171b82be00695c897fb51. No functionality expansion/sourceedit/Cargo. Existing direct focused1/1 proof valid unchanged test2f7a67cf; current candidate graphc2ce9c05 canonical writer449 to606. Earlier disconnected blocker is addressed by actual admission, not policy bypass.'
    - 'decisions: {"decision": "deliver-accepted-candidate", "review_claim_revision": 25, "deliver_claim_revision": 26, "packet": ".forge/evidence/m022-django-review-packet.md", "packet_sha256": "4018cf82d8c3e1fd840771104096c163dbc7965f973c12c380472f404c10efc1", "findings": [], "deferred": [], "source_state": "All six current scope/managed-document and169 production hashes equal independently reviewed binding immediately before delivery.", "permission": "Coordinator authorizes same independent reviewer normal automatic task delivery; no source edits, manual duplicate commit, main integration or publication."}'
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

1. `django.toml` and focused tests cover ORM model/field receivers, querysets, managers, DRF serializers/viewsets, and route declarations.
2. Literal CBV `template_name` and `model` values resolve to the matching template and model only when each has one target; dynamic or missing values do not invent edges.
3. `{% url %}` and `reverse()` resolve literal route names. Tests include missing and duplicate route names and assert unresolved/ambiguous coverage with no edge.
4. `cargo test --test languages` and `cargo clippy --all-targets --all-features -- -D warnings` pass.

---

### task: starlette-and-fastapi-manifests

```yaml
task_ref: starlette-and-fastapi-manifests
target: Add Starlette and FastAPI route, router-prefix, dependency-injection, response-model, and exception-handler rules
proof_policy: seam-test-first
contract_revision: 4
depends_on:
- typed-framework-semantic-linking
- django-and-orm-comprehensive-manifest
scope:
- src/engine/languages/manifests/starlette.toml
- src/engine/languages/manifests/fastapi.toml
- tests/languages/python/frameworks.rs
- docs/reference/framework-manifests.md
- src/db/writer.rs
invariants:
- This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
- src/db/writer.rs is admitted only as an unchanged production proof dependency at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a; engine writes remain forbidden under this Starlette/FastAPI task.
status: completed
subtasks:
- subtask_ref: starlette-route-and-response-rules
  title: Define Starlette Route, Mount, and response class receivers and route calls
  status: completed
  evidence: Exact Starlette project-loaded data proves Route/Mount/WebSocketRoute and JSONResponse/HTMLResponse finite external receiver coverage through one persisted writer/SQLite seam. Handlerless Mount and unknown members remain nonexact. Focused1/0 and final complete languages267/0 strictClippy0; .forge/evidence/m022-starlette-build-packet.md.
- subtask_ref: fastapi-decorator-and-di-rules
  title: Define FastAPI route decorators, APIRouter prefix composition, Depends() injection, response models, and exception handlers
  status: completed
  evidence: Added only manifest call_to_symbol DI and constructed-member symbol_attachment model/exception rules. Shared seam proves exact nested prefixes and4 new canonical joins, missing/duplicate/dynamic/shadow controls, actual effective-rule removal/renamed module/constructor/export/keyword/member/restore, ordinary preservation and final cold/delta facts/coverage/commitments. Focused1/0; complete languages267/0 strictClippy0. No engine edits.
receipt:
  commit: 328fec933ae9208de5ab3773c31755fc2be09f42
  contract_revision: 4
  passed_at: 2026-10-21T01:45:41.131469539+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 267
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All6 scope/managed-manifest hashes equal Build; all5 scope owners equal accepted snapshot37f3c730. Exact4 admitted paths changed; onlyfastapi.toml in src. UnchangedwriterSHA9a5684b13 andStarlette data preserved; evidence-only untrackedpaths.
        claims:
          applicable: true
          evidence: 'AC1-6: finite13manifestsemanticentries through acceptedgenericoperations; same shared writer/reopenedSQLite seam proves4 formerlymissingcanonicaljoins, nestedprefix/route/response controls, missing/duplicate/dynamic/shadow noedges and6 realrule/source states with ordinarycall/import preservation, facts/coverage/commitments/cold-delta parity. Independent strictClippy0 full languages267/0. No finding. Packet .forge/evidence/m022-starlette-review-packet.md SHA256 db98cc54c63d29bb98fe7d5c443395ee0cb6c895dcdb17d639988c300cd92672'
        concurrency:
          applicable: true
          evidence: No concurrency/transaction/lock/publication code changes; allproductionRust preserved. SoleCargo scheduler with terminal command. Fresh lifecycle claim11/contract4/candidate fence; no direct database or receipt mutation.
        project_isolation:
          applicable: true
          evidence: Dedicated Starlette root; fixture-local project manifests/SQLite and current snapshot. Existingworkspace/target canonical rules retained. Warm Django target used only for isolated sequential Cargo artifactreuse. No linkedworkspace/main/remote changes.
        administration:
          applicable: true
          evidence: Independentcodex-sol-starlette-reviewer vs acceptedbuildercodex-sol-builder; approved unchangedwriter proofboundary. Genuine4missingjoinRED retained; one unchangedsharedauthoredtest, no perdefecttest. Bundle/packagefile registration remains named successorAC4. No manualcommit/profile/receipt/engine changes; same reviewer automaticdelivery authorized.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: A fixed in-repository framework fixture measures resolved / (resolved + ambiguous + unresolved), excluding external imports. The candidate must exceed both the measured branch-point baseline and 85%; only coverage counts and percentage are acceptance evidence. Timing, memory, and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
    - src/db/writer.rs is admitted only as an unchanged production proof dependency at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a; engine writes remain forbidden under this Starlette/FastAPI task.
    architectural_notes:
    - 'decisions: {"decision": "deliver-reviewed-frozen-candidate", "findings": [], "deferred": [], "review_claim_revision": 11, "deliver_claim_revision": 12, "packet_sha256": "db98cc54c63d29bb98fe7d5c443395ee0cb6c895dcdb17d639988c300cd92672", "source": "All6boundfiles/169src unchanged immediately beforedelivery; normal auto_commit authorized. No mainintegration or publication.", "successor_obligations": "Bundle registration/package-file activation AC4 and fixedcoverage percentage remain explicit named successor obligations, not this task proof."}'
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

1. `starlette.toml` and `fastapi.toml` cover `Route`, `Mount`, `WebSocketRoute`, response classes, route decorators, and `APIRouter` prefix composition.
2. `Depends()` references, declared response models, and exception handlers produce only exact typed relations supported by extracted facts.
3. Tests cover nested prefixes, literal and dynamic paths, missing targets, duplicate targets, response models, and exception handlers; non-unique cases emit no edge.
4. Bundle registration and supported Python package-file activation are verified by `framework-bundle-registration`.
5. `cargo test --test languages` and strict Clippy pass.
6. Prove pack removal and unrelated API renaming through persisted results without changing production Rust. Use indexed local targets for exact model/exception relations and retain external coverage for unindexed third-party classes. Keep nested-prefix proof in the domain suite.

---

### task: nuxt-and-vue-pinia-manifests

```yaml
task_ref: nuxt-and-vue-pinia-manifests
target: Add Nuxt 3 pages/composables and Vue 3 SFC/Pinia rules with exact component and store references
proof_policy: direct-proof
contract_revision: 5
depends_on:
- typed-framework-semantic-linking
scope:
- src/engine/languages/manifests/vue-runtime.toml
- src/engine/languages/manifests/nuxt.toml
- tests/languages/typescript/frameworks.rs
- docs/reference/framework-manifests-nuxt-vue-pinia.md
- src/db/reader.rs
invariants:
- This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
- src/db/reader.rs is admitted only as an unchanged production proof dependency at SHA-256 70f4682598f270c1bcd9e72090024d2c8c99bcfb1a20d863a281dcb308181ac6; engine writes remain forbidden. Keep the real writer pipeline and use its reopened reader for the indexed seam.
status: completed
subtasks:
- subtask_ref: nuxt-pages-and-composables-rules
  title: Define Nuxt 3 page-to-route resolution, useFetch, and useAsyncData composable bindings
  status: completed
  evidence: Nuxt canonical TOML configures static pages file_route and useFetch/useAsyncData autoimports. Same shared real writer/reopened reader matrix passes pages/composables original+renamed, package activation, replace/restore, provider mutation, exact routes and cold-delta parity. Focused1/0; registered strict Clippy0 plus languages268/0. Logs .forge/m022-nuxt-build-post-generic-{focused,final}.log.
- subtask_ref: vue-sfc-and-pinia-store-rules
  title: Define Vue 3 component props/emits and Pinia defineStore state and action receivers
  status: completed
  evidence: Canonical vue-runtime TOML configures imported Pinia options factory_members preserving existing SFC rules. Same shared matrix passes actual TS and Vue state/action declarations, props/emits and unique components, persisted provenance, mutation/activation/replace and cold-delta parity both spellings after generic7cae325 integration. Focused1/0; registered strict Clippy0 plus languages268/0. No engine writes.
- subtask_ref: vue-component-and-store-negative-cases
  title: Cover missing and duplicate Vue component and Pinia store identities
  status: completed
  evidence: Same single pack matrix proves missing/duplicate component identities and dynamic/missing/shadowed/duplicate/rebound store nonexact controls, ordinary call/import preservation, renamed rules and cached replay. No generic codec/stress duplication or new per-defect test functions. Focused1/0 and registered strict Clippy0 plus languages268/0; exact unchanged reader dependency retained.
receipt:
  commit: 635d2889d18b63d698e043dbfff71af676331719
  contract_revision: 5
  passed_at: 2026-10-21T03:33:33.783732040+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 268
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Exactly two admitted TOML tables, one existing TS domain module, own new reference page and managed authority amendment; all6candidate/169src hashes verified; all5scope owners match snapshot2903f2c4. Reader unchanged, no engine writes. Prior authority-refresh archives preserved.
        claims:
          applicable: true
          evidence: 'Current rev5 direct-proof AC1-5: four actual pack families original/renamed through real writer/reopenedreader, canonical page/component/store targets, SFC props/emits, both composables, precision/provenance, source/provider/package/rule transitions and cold-delta preservation. Independent strictClippy0 and full languages268/0. No finding. Packet .forge/m022-nuxt-r5-review-packet.md SHA256 f5a5aaae0bafbf7841f82a8deb5e4520bb82be26cf6389ba2d1f1f5fcf06c775'
        concurrency:
          applicable: true
          evidence: No transaction/lock/publication/revision/cache implementation changes; existing engine bytes preserved. Reviewer Cargo waits for explicit root slot transfer and runs alone. Fresh claim9/contract5/baseline7cae325 and supported candidate fencing retained.
        project_isolation:
          applicable: true
          evidence: Dedicated Nuxt root, fixture-local package/manifests/SQLite. Plain nested project and component-collision controls preserve own provenance. Canonical vue-runtime identity reused; generic prerequisite handles language semantics. No other worktree source integration or main/remote changes.
        administration:
          applicable: true
          evidence: Independent codex-sol-nuxt-reviewer vs codex-sol-builder; pinned taskfa664/graphcc869 hashes and current canonical reader305/writer606 witness verified. Rev5 metadata-only direct-proof refresh honestly preserves real priorGREEN and historicalRED/failedBuild; no fake execution. AC4 registration/package-file matrix remains named successor. Same reviewer authorized automaticdelivery.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: A fixed in-repository framework fixture measures resolved / (resolved + ambiguous + unresolved), excluding external imports. The candidate must exceed both the measured branch-point baseline and 85%; only coverage counts and percentage are acceptance evidence. Timing, memory, and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
    - src/db/reader.rs is admitted only as an unchanged production proof dependency at SHA-256 70f4682598f270c1bcd9e72090024d2c8c99bcfb1a20d863a281dcb308181ac6; engine writes remain forbidden. Keep the real writer pipeline and use its reopened reader for the indexed seam.
    architectural_notes:
    - 'decisions: {"decision": "deliver-current-reviewed-Nuxt-rev5", "findings": [], "deferred": [], "review_claim_revision": 9, "deliver_claim_revision": 10, "packet_sha256": "f5a5aaae0bafbf7841f82a8deb5e4520bb82be26cf6389ba2d1f1f5fcf06c775", "source": "All6candidate/169src and historical archive/pinnedtool bytes preserved. Normal automatic delivery authorized; no manualcommit or mainintegration."}'
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

1. `nuxt.toml` and `vue-runtime.toml` cover pages under `pages/**/*.vue`, `useFetch`, `useAsyncData`, `<script setup>`, `defineProps`, and `defineEmits`.
2. Tests cover page-path mapping and both composables, SFC props/emits, and `defineStore` state/action references, including missing and duplicate targets.
3. Only static, uniquely supported component and store identities produce edges; dynamic identities remain unresolved.
4. Bundle registration and supported JavaScript package-file activation are verified by `framework-bundle-registration`.
5. `cargo test --test languages` and strict Clippy pass.

---

### task: htmx-and-alpine-cross-language-linking

```yaml
task_ref: htmx-and-alpine-cross-language-linking
target: Link HTMX requests and Alpine stores/events across language-owned facts with exact targets
proof_policy: direct-proof
contract_revision: 5
depends_on:
- django-and-orm-comprehensive-manifest
- starlette-and-fastapi-manifests
scope:
- src/engine/languages/manifests/htmx.toml
- src/engine/languages/manifests/alpinejs.toml
- tests/languages/html/framework_relations.rs
- tests/languages/manifests.rs
- docs/reference/framework-manifests-htmx-alpine.md
- src/db/writer.rs
invariants:
- This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
- src/db/writer.rs is admitted only as an unchanged production proof dependency at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a; engine writes remain forbidden. Keep cross-language JavaScript fixtures in the shared HTML suite so the Nuxt task retains exclusive TypeScript-suite ownership.
- tests/languages/manifests.rs may change only to isolate the existing named_binding generic fixture from bundled Alpine semantics, preserving all five operation, unrelated-renaming, rule-causality and cold/delta proofs. No new test functions, weakened assertions, engine changes or generic capability changes are admitted.
status: completed
subtasks:
- subtask_ref: htmx-endpoint-triggers-rules
  title: Resolve literal local hx-get/post/put/patch/delete URLs to backend routes; retain hx-target and hx-swap as context
  status: ready
  evidence: null
- subtask_ref: alpine-store-and-dispatch-rules
  title: Resolve Alpine $dispatch events to x-on listeners and $store names to unique declarations
  status: ready
  evidence: null
receipt:
  commit: ddb3281092fbcbef7f05c7297b1399d0f95e56fa
  contract_revision: 5
  passed_at: 2026-10-21T03:48:48.338431725+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 268
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Exact six owners plus approved managed authority amendment; all7candidate/169src hashes frozen. Only4Alpine rules, one HTML contract test with2same-author setup fixes,6line generic fixture isolation and own doc. Writer and HTMX TOML unchanged. Prior27evidence inputs+7archives preserved.
        claims:
          applicable: true
          evidence: 'Currentrev5/direct-proof AC1-5: ten actual route/handler identities and context; URL failclosed; actual unique/missing/duplicate/shadow store/event targets and exactcoverage; no package manifests; provider/rule/source renaming and workspaceisolation; realwriter/reopenedSQLite/cold-delta. IndependentClippy0/languages268/0, fmt/diff0. No findings. Packet SHA256 66e548daf48f8bee8319eb83290a9d830b7a8fbe8feb81966e98676acc5b2e62'
        concurrency:
          applicable: true
          evidence: No lock/transaction/cache/schema/publication implementation changes. FreshReview9 currentcontract5 candidate fencing preserved; soleCargo transferred and released at terminal. Interruptedtool124 archived honestly, no count; no child remained before one identical restart. No further Cargo.
        project_isolation:
          applicable: true
          evidence: DedicatedHTMX root and fixture-local SQLite/manifests. Independentworkspace proves scopedreplacement does not escape. Genericnamed_binding disables bundledAlpine only inside its own workspace; otherassertions and all5families unchanged. No activeNuxt source writes or HEADintegration.
        administration:
          applicable: true
          evidence: Independent codex-sol-htmx-reviewer distinct from codex-sol-htmx-builder. Pinnedtaskfa664/graphcc869 hashes verified; currenttestline52->writer606 witness exact. Acceptedrev5direct-proof/history preserved without fakeRED or falsecounts. SameReviewer normalautocommit authorized; no manualreceipts/main/publication.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: A fixed in-repository framework fixture measures resolved / (resolved + ambiguous + unresolved), excluding external imports. The candidate must exceed both the measured branch-point baseline and 85%; only coverage counts and percentage are acceptance evidence. Timing, memory, and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
    - src/db/writer.rs is admitted only as an unchanged production proof dependency at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a; engine writes remain forbidden. Keep cross-language JavaScript fixtures in the shared HTML suite so the Nuxt task retains exclusive TypeScript-suite ownership.
    - tests/languages/manifests.rs may change only to isolate the existing named_binding generic fixture from bundled Alpine semantics, preserving all five operation, unrelated-renaming, rule-causality and cold/delta proofs. No new test functions, weakened assertions, engine changes or generic capability changes are admitted.
    architectural_notes:
    - 'decisions: {"decision": "deliver-current-reviewed-HTMXrev5", "findings": [], "deferred": [], "review_claim_revision": 9, "deliver_claim_revision": 10, "packet_sha256": "66e548daf48f8bee8319eb83290a9d830b7a8fbe8feb81966e98676acc5b2e62", "source": "All7candidate/169src+27boundevidence+7archivebytes preserved. No product edit afterReviewPASS; normal automaticdelivery only."}'
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

1. Literal local HTMX request URLs link to matching Django and FastAPI routes for supported HTTP methods. Remote, dynamic, empty, and current-page values do not create endpoint edges.
2. `hx-target` and `hx-swap` are retained as request context and are never interpreted as endpoint identities.
3. Alpine `$dispatch` links to matching `x-on` listeners and `$store.<name>` links to one declared store. Missing or duplicate listeners/stores retain unresolved or ambiguous coverage and emit no edge.
4. The same explicit HTMX and Alpine facts work without package manifests; they do not depend on an unsupported marker-activation schema. Workspace-local manifest overrides remain scoped to their workspace.
5. `cargo test --test languages` and strict Clippy pass.

---

### task: gridviewspec-full-manifest-pack

```yaml
task_ref: gridviewspec-full-manifest-pack
target: Define typed GridViewSpec component and Python/Alpine/HTMX bindings
proof_policy: direct-proof
contract_revision: 5
depends_on:
- django-and-orm-comprehensive-manifest
- htmx-and-alpine-cross-language-linking
scope:
- src/engine/languages/manifests/gridviewspec.toml
- tests/languages/python/frameworks.rs
- tests/languages/html/framework_relations.rs
- tests/languages/manifests.rs
- docs/reference/framework-manifests.md
- docs/reference/framework-manifests-gridviewspec.md
- src/db/writer.rs
invariants:
- This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
- src/db/writer.rs is admitted only as an unchanged production proof dependency at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a; engine writes remain forbidden.
- tests/languages/manifests.rs may change only to isolate the existing generic named_binding fixture from bundled Alpine and GridViewSpec named-binding semantics. Preserve all five operations, unrelated-renaming, provenance, raw-vector rule-removal and cold/delta assertions; no new test functions, weakened assertions, engine changes or pack-policy changes are admitted.
status: completed
subtasks:
- subtask_ref: gridviewspec-component-and-action-receivers
  title: Define typed rules for GridViewSpec tables, toolbars, filters, cards, charts, KPIs, tabs, and actions
  status: completed
  evidence: Preserved28receiver catalogue and8component family evidence/unknown controls. Actual renderer/action callbacks use isolated gridviewspec_renderer/gridviewspec_action domains with missing/duplicate/dynamic/shadow/rebound nonexact controls. Formatted Gridfocused1/0 and existinggenericall5ops1/0; final strictClippy0+languages270/0 after removing needlessborrow. No engine writes/newtestfunctions. Logs .forge/evidence/m022-gridviewspec-r5-focused.log and m022-gridviewspec-r5-final-pass.log.
- subtask_ref: gridviewspec-backend-and-template-semantic-rules
  title: Add semantic rules linking GridViewSpec classes to backend Python datasources and frontend Alpine/HTMX controllers
  status: completed
  evidence: TOML-only direct nested GridViewTable.datasource/GridViewDataSource endpoint_projection binds actual unique GET/POST routes; imported GridViewColumn renderer/extra.action joins actual JS callback declarations. Real HTML/static/dynamic controls and ordinary imports/calls retained. Final formatted focused2/0 and registeredstrictClippy0+languages270/0, source-bound canonical writer606 witness. No inventedcontrollerAPI or operation.
- subtask_ref: gridviewspec-workspace-overrides
  title: Cover GridViewSpec workspace-local extension and replacement behavior
  status: completed
  evidence: Same shared Gridseam passes workspace extension/catalogue preservation, rule remove/replace/restore, independentworkspace isolation, unrelated API/keyword rename and provider/consumer delta with exact persistednodes/facts/coverage/commitments and coldparity. Existinggenericnamed_binding fixture alone isolates Alpine+Grid bundled rules; all rawvector/provenance/assertions unchanged. FinalGrid1/0+generic1/0,Clippy0+languages270/0.
receipt:
  commit: 12aa3e62a548500c8d52bb646b8d28486ae31ed6
  contract_revision: 5
  passed_at: 2026-10-21T04:43:25.679404832+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 270
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Currentrev5 exact7TaskSpec owners/8candidate-managed paths and169src bytebound. Only5Grid finite data rules, one appended Python contract test, exact generic named_binding override loop and own doc; centralreference/HTML/writer unchanged. All prior Python bytes and generic assertions preserved,31inputs+10archives retained.
        claims:
          applicable: true
          evidence: 'AC1-4: all8componentfamilies and28receivers; exact imported nested datasourceGET/POST and actual callback targets through unique globalalias; missing/duplicate/dynamic/shadow/rebound failclosed; rule/provider/consumer changes, unrelatednames, extension/replacement and ownerisolation. Realwriter/reopenedreader/commitment path and cold-delta facts/nodes/coverage. IndependentClippy0+languages270/0; precommandfmt/diff0. No findings. Packet SHA256 bd3f013a0c0f8891829b0e99c1fa70abb96cab3420481a4453a08abb8e0fb46e'
        concurrency:
          applicable: true
          evidence: Transaction/lock/cache/schema/publication code unchanged. FreshReview9 contract5 candidate fencing preserved. SoleCargo transferred in sameGridroot; explicitwarmtarget/jobs2/offline and lifetime3600000. No mtime invalidation, duplicatecommand or restart; Cargo released at terminal. No activeHEADintegration.
        project_isolation:
          applicable: true
          evidence: DedicatedGridroot and fixture-local manifests/SQLite. Extension retains28receivers; emptyreplacement loses configuredreceivers while neighboringworkspace retainsbundledcoverage. Generic isolation only existing named_binding case and finite-policy controls; actualGrid/HTMX bundle proofs retained.
        administration:
          applicable: true
          evidence: Independent codex-sol-gridviewspec-reviewer vs codex-sol-builder. Pinnedtaskfa664/graphcc869 verified; currentcanonical writer606/reader305/verify750 witnesses exact. Genuine historical failures/metadata refresh preserved honestly, actualformattedcandidate independentlytested. All3live subtasks completed beforefreeze. SameReviewer normalautocommit authorized; no manualreceipts/main/publication.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: A fixed in-repository framework fixture measures resolved / (resolved + ambiguous + unresolved), excluding external imports. The candidate must exceed both the measured branch-point baseline and 85%; only coverage counts and percentage are acceptance evidence. Timing, memory, and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - This pack contains declarative data only; engine capability gaps return to typed-framework-semantic-linking.
    - src/db/writer.rs is admitted only as an unchanged production proof dependency at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a; engine writes remain forbidden.
    - tests/languages/manifests.rs may change only to isolate the existing generic named_binding fixture from bundled Alpine and GridViewSpec named-binding semantics. Preserve all five operations, unrelated-renaming, provenance, raw-vector rule-removal and cold/delta assertions; no new test functions, weakened assertions, engine changes or pack-policy changes are admitted.
    architectural_notes:
    - 'decisions: {"decision": "deliver-reviewed-Gridrev5", "findings": [], "deferred": [], "review_claim_revision": 9, "deliver_claim_revision": 10, "packet_sha256": "bd3f013a0c0f8891829b0e99c1fa70abb96cab3420481a4453a08abb8e0fb46e", "source": "All8candidate/169src+31boundinputs+10archives preserved. No product edits afterReviewPASS; all3subtasks alreadycompleted; normalautocommit only."}'
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

1. `gridviewspec.toml` covers tables, toolbars, filters, cards, charts, KPIs, tabs, and actions consistently.
2. Typed rules link declared Python datasources and Alpine/HTMX controllers only when their identities resolve uniquely.
3. Tests cover each component family, missing targets, duplicate targets, and workspace-local `extend` / `replace`; non-unique targets emit no edge.
4. `cargo test --test languages` and strict Clippy pass.

---

### task: framework-coverage-verification

```yaml
task_ref: framework-coverage-verification
target: Improve observed Commerce and Library framework classifications using existing declarative rules and measure actual before/after identities
proof_policy: direct-proof
contract_revision: 5
depends_on:
- typed-framework-semantic-linking
- django-and-orm-comprehensive-manifest
- starlette-and-fastapi-manifests
- nuxt-and-vue-pinia-manifests
- htmx-and-alpine-cross-language-linking
- gridviewspec-full-manifest-pack
- framework-bundle-registration
scope:
- src/engine/languages/manifests/alpinejs.toml
- src/engine/languages/manifests/gridviewspec.toml
- src/engine/languages/manifests/django.toml
- src/engine/languages/manifests/starlette.toml
- src/engine/languages/manifests/nuxt.toml
- src/db/writer.rs
invariants:
- Latest explicit user authority selects actual Commerce and Library observations and universal TOML data only. No synthetic tests, new fixtures, Rust changes, invented APIs, new operations, or numeric coverage promise are admitted. The protected historical milestone header remains unchanged; its synthetic 85-percent experiment is not claimed by this actual-data task.
- Only the five scoped TOML packs may change behavior. src/db/writer.rs is an unchanged production proof dependency retained from revision4 (SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a); no edits are admitted to it. Preserve existing schema, package/configuration eligibility, filename identities, merge/replace/enabled semantics, and manifest-sensitive cache binding.
- Freeze the three m022-commerce-real-patterns source/SQL packets before Build. Add only finite observed names supported by existing operations. FastAPI, shortcuts and DRF have no observed missing data and receive no speculative entries. Unsupported dynamic chains, cross-scope joins, setup-store and callable-return inference remain explicit gaps.
- Build one consolidated embedded candidate and measure each real control separately against its retained baseline with unchanged source bytes and adapter. Do not reindex the Commerce baseline, edit live origins, pad counts, suppress negatives, or equate local full replacements with compiled defaults. An optional minimal enabled Alpine project manifest in the isolated copy is an explicitly separate configuration effect, never silent bundle equivalence.
- Report complete persisted R/A/U/E counts and per-rule identity transitions, distinguishing exact R gains, U-to-E classification and activation-only effects. E is not R. Retain raw policy exits and the existing HTML dynamic/Python ambiguity precision exceptions. Timing/RSS/size are not gates; no 1000-edge or percentage threshold is promised.
status: completed
subtasks:
- subtask_ref: deterministic-framework-coverage-fixture
  title: Apply one source-bound universal rule-data batch from frozen actual-control findings
  status: completed
  evidence: Five bundled TOMLs exact finite-union preserved; focused seam1/0; strictClippy0/languages271/0; actual frozen embedded candidate control results and complete identity/edge/source/config proof at /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-final-framework-data-build/packet.json SHA256 acfd8abd52d5ac3cdbd91a7ef882e75b8c2b6e769a6de5e3f3b6eca59c9d7930
- subtask_ref: coverage-only-baseline-comparison
  title: Measure Commerce and Library separately with retained baselines and exact per-rule R/A/U/E transitions
  status: completed
  evidence: Five bundled TOMLs exact finite-union preserved; focused seam1/0; strictClippy0/languages271/0; actual frozen embedded candidate control results and complete identity/edge/source/config proof at /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-final-framework-data-build/packet.json SHA256 acfd8abd52d5ac3cdbd91a7ef882e75b8c2b6e769a6de5e3f3b6eca59c9d7930
receipt:
  commit: bdfcdbb1775b0350030506bb1d55a177534ffbd0
  contract_revision: 5
  passed_at: 2026-10-21T16:42:56.663963664+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 271
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Only five admitted TOMLs change; all six snapshot paths exact, writer unchanged.179 compiled/107 tests frozen;287 review inputs unchanged. Header10 taskblocks parent-exact before generated receipt. validation.json. /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-final-framework-data-review10/packet.md SHA256 d7735ad3af02d928212d3fad29954675bb1c9c4084f12b60dab62858bbe2c873
        claims:
          applicable: true
          evidence: All five files/20 groups reviewed; existing production loader+embedded writer and actual control packet validate finite data. Independent exact Clippy0+languages271/0 exit0, logSHA01f32b44bc152a2c50f9b763efc546921ce6834d7a807bce9c5b49572b651775. Primary006 no R/edge/identity loss;7E-to-U and327E-origin changes honestly source-adjudicated.138 cumulative edges and16precision exceptions retained. /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-final-framework-data-review10/packet.md SHA256 d7735ad3af02d928212d3fad29954675bb1c9c4084f12b60dab62858bbe2c873
        concurrency:
          applicable: true
          evidence: No Rust/task transactions/locks/cache/schema changes. Explicit current root, independent claim10/rev5/digest and immutable snapshot/parent fenced; one required command terminal and Cargo released. /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-final-framework-data-review10/packet.md SHA256 d7735ad3af02d928212d3fad29954675bb1c9c4084f12b60dab62858bbe2c873
        project_isolation:
          applicable: true
          evidence: All data and candidate control copies scoped. Seven masking local overrides removed; minimal Alpine activation explicit separate effect. No global activation/renderer join/local-name guess, live-source edit, main or publication. /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-final-framework-data-review10/packet.md SHA256 d7735ad3af02d928212d3fad29954675bb1c9c4084f12b60dab62858bbe2c873
        administration:
          applicable: true
          evidence: Distinct builder/reviewer, current direct-proof authority and both supported-completed subtasks verified. Profiles/hooks/179compiled+107tests/proof history intact. Same reviewer supported Review→Deliver auto_commit only; no manual receipt/commit or additional gate runs. /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-final-framework-data-review10/packet.md SHA256 d7735ad3af02d928212d3fad29954675bb1c9c4084f12b60dab62858bbe2c873
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: A fixed in-repository framework fixture measures resolved / (resolved + ambiguous + unresolved), excluding external imports. The candidate must exceed both the measured branch-point baseline and 85%; only coverage counts and percentage are acceptance evidence. Timing, memory, and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - Latest explicit user authority selects actual Commerce and Library observations and universal TOML data only. No synthetic tests, new fixtures, Rust changes, invented APIs, new operations, or numeric coverage promise are admitted. The protected historical milestone header remains unchanged; its synthetic 85-percent experiment is not claimed by this actual-data task.
    - Only the five scoped TOML packs may change behavior. src/db/writer.rs is an unchanged production proof dependency retained from revision4 (SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a); no edits are admitted to it. Preserve existing schema, package/configuration eligibility, filename identities, merge/replace/enabled semantics, and manifest-sensitive cache binding.
    - Freeze the three m022-commerce-real-patterns source/SQL packets before Build. Add only finite observed names supported by existing operations. FastAPI, shortcuts and DRF have no observed missing data and receive no speculative entries. Unsupported dynamic chains, cross-scope joins, setup-store and callable-return inference remain explicit gaps.
    - Build one consolidated embedded candidate and measure each real control separately against its retained baseline with unchanged source bytes and adapter. Do not reindex the Commerce baseline, edit live origins, pad counts, suppress negatives, or equate local full replacements with compiled defaults. An optional minimal enabled Alpine project manifest in the isolated copy is an explicitly separate configuration effect, never silent bundle equivalence.
    - Report complete persisted R/A/U/E counts and per-rule identity transitions, distinguishing exact R gains, U-to-E classification and activation-only effects. E is not R. Retain raw policy exits and the existing HTML dynamic/Python ambiguity precision exceptions. Timing/RSS/size are not gates; no 1000-edge or percentage threshold is promised.
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

1. The five scoped packs contain only the frozen actual-data additions: finite Alpine magic receivers, proven Grid globals/renderer registration identities, Django Form/ModelForm inherited members, Starlette literal Route methods and verified middleware member, and observed Nuxt helper/configured module component names. Preserve all existing pack behavior and fail-closed guards; record nonexpressible cases without inventing rules.
2. Existing production loader/schema paths validate the complete TOML batch. No new synthetic tests or fixtures are added or run for this task; existing suite coverage is retained. All Rust, documentation and unrelated data remain unchanged.
3. One rebuilt embedded candidate is compared against the retained Commerce baseline and the separately bound Library baseline on their respective fixed real inputs. Record binary/source/configuration bindings, all language/framework counts and exact per-rule transitions, including residual U/A and precision exceptions. Separate any explicit isolated-copy Alpine activation from new-rule effects.
4. Record observed classification changes honestly: external/builtin classification is E, not an exact edge. Preserve original raw results and policy exits; no claimed 85-percent result, synthetic-corpus result, performance gate or manufactured RED.
5. Independent review covers the complete five-file batch once; the supported Build evidence records production-loader validation and the actual single-candidate comparison. Complete both advisory subtasks with their actual evidence before Review freeze.

---

### task: framework-bundle-registration

```yaml
task_ref: framework-bundle-registration
target: Register the new bundled packs and verify dependency-scoped and workspace-local activation
proof_policy: seam-test-first
contract_revision: 6
depends_on:
- django-and-orm-comprehensive-manifest
- starlette-and-fastapi-manifests
- nuxt-and-vue-pinia-manifests
- htmx-and-alpine-cross-language-linking
- gridviewspec-full-manifest-pack
scope:
- build.rs
- src/engine/languages/dependency_registry.rs
- src/engine/languages/manifests/framework.rs
- tests/languages/manifests.rs
- docs/reference/framework-manifests.md
invariants:
- Embed supported manifest files automatically from src/engine/languages/manifests/ at build time; derive each identity from its filename stem and retain embedded defaults. No handwritten framework identity list, new runtime search path, configuration, dependency, framework-specific branch or extraction capability is admitted.
- Use deterministic sorted inventory and the existing TOML/YAML/YML format policy. Duplicate identities and invalid admitted inputs fail closed. Track directory and file add/remove/edit inputs through Cargo invalidation and the complete bundled-content fingerprint used by existing cache identity.
- Preserve workspace-local arbitrary-filename discovery, project-first default merge, extend=true, explicit replace=true including empty replacement, dependency activation and workspace isolation. Documentation scope covers the automatic embedded inventory, activation/merge behavior and explicit enabled control only.
- Support optional strictly boolean enabled, defaulting to true when omitted. A workspace-local enabled=false is a whole-pack tombstone for that filename identity, suppressing all bundled and project rule families regardless of merge or replacement; never fall back to its bundled defaults. Explicit true or removal restores ordinary eligibility. Preserve validated manifest shape, serialization, input hashing, existing removal diagnostics and cold/delta consistency. Use enabled=false for generic fixture pack isolation rather than presenting empty replacement as a disable mechanism.
status: completed
subtasks:
- subtask_ref: register-new-bundled-manifests
  title: Automatically embed filename-identified manifest files, including Starlette, preserving existing vue-runtime and HTMX identities
  status: completed
  evidence: 'Build9 rev6: exact shared registration seam 1passed/0failed/270filtered; compiled buildscript inventory sensitivity PASS8phases; registered strictClippy exit0 + full languages271passed/0failed. Logs and hashes under .forge/evidence/m022-registration-build-*; all pack disable/restore/local precedence/cold-delta phases completed.'
- subtask_ref: verify-package-and-workspace-activation
  title: Verify package declarations and workspace-local extend/replace behavior for all framework packs
  status: completed
  evidence: 'Build9 rev6: exact shared registration seam 1passed/0failed/270filtered; compiled buildscript inventory sensitivity PASS8phases; registered strictClippy exit0 + full languages271passed/0failed. Logs and hashes under .forge/evidence/m022-registration-build-*; all pack disable/restore/local precedence/cold-delta phases completed.'
receipt:
  commit: b2874ef859f894bec73ca9fdeff10bb647016fb6
  contract_revision: 6
  passed_at: 2026-10-21T05:36:16.140032912+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 271
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Exact5admittedowners+unchangedacceptedcoordinatoramendment,6candidate paths,168productioninventory+all169src bytebound. Only generator/registry/strictmanifestmodel/test/reference edits. No new dependency/cache/runtimepath/extractor/frameworkbranch.17boundevidence inputs retained.
        claims:
          applicable: true
          evidence: 'Currentrev6AC1-4 and4explicitinvariants: deterministicautomaticfilename inventory+pathbytesfingerprint, actualcompiledgenerator8phase artifact bound to finalsource/binary; strictdefaulttrue/serializedfalse and localtombstone beforefallback/filter acrossallfamilies; validatedshape/diagnostics/inputdigest retained.8realpackrows,nearestscope/all8supportedpackagefiles,overrideformats/modes,nonemptypayloaddisable/restore,cold-delta throughactualregistry/writer. IndependentClippy0; languages271/0; requiredlib {''passed'': 70, ''failed'': 0, ''ignored'': 0, ''measured'': 0, ''filtered'': 0}. No findings. PacketSHA256 2dbf2e1c97859fe3717c1e5be376fe58845cd57ac636aefed8674097533fcbe6'
        concurrency:
          applicable: true
          evidence: No transaction/lock/publication implementation change. Existingcacheidentity receivesfullrelativefilenames+bytes; Cargodirectory/file tracking retained. FreshReview10 currentcontract6/snapshot fences intact. SoleCargowrapper sequential languages thenrequiredlib, one strictalltargetsClippy; no switchtouch/duplicate/restart or activeHEADintegration.
        project_isolation:
          applicable: true
          evidence: Existing arbitrarylocalfilename discovery,projectfirstmerge/extend/replace,nearestpackage/workspace resolution unchanged. Disabledlocalmapentry retainedbeforeenabledfilter, so no bundledfallback; ordinarycalls/imports and unrelatedworkspaces remain. Existing symlink/size/strictshape validation retained, generator rejects admittedsymlinks/duplicate/nonUTF8identity.
        administration:
          applicable: true
          evidence: Independent codex-sol-registration-reviewer vs codex-sol-builder; pinnedtaskfa664/graphcc869 verified. Canonicaltest23->registry348/writer606 currentwitness retained. GenuineContract seam/history and firstClippy-onlyfailure preserved; actualfinalbytes independentlytested. Bothliveadvisorysubtasks completedbeforefreeze; sameReviewer supportednormalautocommit only.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: A fixed in-repository framework fixture measures resolved / (resolved + ambiguous + unresolved), excluding external imports. The candidate must exceed both the measured branch-point baseline and 85%; only coverage counts and percentage are acceptance evidence. Timing, memory, and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - Embed supported manifest files automatically from src/engine/languages/manifests/ at build time; derive each identity from its filename stem and retain embedded defaults. No handwritten framework identity list, new runtime search path, configuration, dependency, framework-specific branch or extraction capability is admitted.
    - Use deterministic sorted inventory and the existing TOML/YAML/YML format policy. Duplicate identities and invalid admitted inputs fail closed. Track directory and file add/remove/edit inputs through Cargo invalidation and the complete bundled-content fingerprint used by existing cache identity.
    - Preserve workspace-local arbitrary-filename discovery, project-first default merge, extend=true, explicit replace=true including empty replacement, dependency activation and workspace isolation. Documentation scope covers the automatic embedded inventory, activation/merge behavior and explicit enabled control only.
    - Support optional strictly boolean enabled, defaulting to true when omitted. A workspace-local enabled=false is a whole-pack tombstone for that filename identity, suppressing all bundled and project rule families regardless of merge or replacement; never fall back to its bundled defaults. Explicit true or removal restores ordinary eligibility. Preserve validated manifest shape, serialization, input hashing, existing removal diagnostics and cold/delta consistency. Use enabled=false for generic fixture pack isolation rather than presenting empty replacement as a disable mechanism.
    architectural_notes:
    - 'decisions: {"decision": "deliver-reviewed-registration-rev6", "findings": [], "deferred": [], "review_claim_revision": 10, "deliver_claim_revision": 11, "packet_sha256": "2dbf2e1c97859fe3717c1e5be376fe58845cd57ac636aefed8674097533fcbe6", "source": "All6candidate/168production+169src preserved; no product edit afterReviewPASS; normalautocommit only."}'
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

1. All eight bundles are available through the standard framework loader, including the new Starlette, Vue, and HTMX bundles.
2. Package activation tests use supported Python declarations (`pyproject.toml`, `requirements*.txt`, `setup.cfg`, `Pipfile`) and JavaScript declarations (`package.json`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`).
3. Tests prove package scoping, no activation from unrelated workspaces, and workspace-local `.forge/frameworks/<filename-stem>.toml|yaml|yml` merge/replace behavior.
4. `cargo test --test languages` and strict Clippy pass.

---

### task: framework-manifest-authoring-documentation-and-guides

```yaml
task_ref: framework-manifest-authoring-documentation-and-guides
target: Comprehensive documentation, schema specification, authoring guides, annotated examples in docs/reference/framework-manifests.md and skills/contextunity-forge/, and example syntax validation test
proof_policy: seam-test-first
contract_revision: 4
depends_on:
- typed-framework-semantic-linking
- django-and-orm-comprehensive-manifest
- starlette-and-fastapi-manifests
- nuxt-and-vue-pinia-manifests
- htmx-and-alpine-cross-language-linking
- gridviewspec-full-manifest-pack
- framework-bundle-registration
- framework-coverage-verification
scope:
- docs/reference/framework-manifests.md
- docs/reference/README.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/frameworks/
- docs/runbooks/
- tests/languages/manifests.rs
- src/db/writer.rs
invariants:
- src/db/writer.rs is an unchanged strict production-loader proof dependency only, at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a. No source edits, visibility changes, wrappers or engine behavior are admitted; preserve all existing guide requirements and aggregate example validation.
status: completed
subtasks:
- subtask_ref: manifest-schema-and-structure-documentation
  title: Document the filename-derived ID and actual root fields, typed receiver/route tables, and semantic rule schema
  status: completed
  evidence: Central reference documents strict filename/root schema, supported dependency/configuration inputs, merge/replace/extend/enabled tombstone, imported/optional attachment and configured factory/result uncertainty; links/frontmatter/TOML checks PASS.
- subtask_ref: how-to-add-new-framework-pack-guide
  title: Explain package activation, workspace overrides, language-owned fact extraction, existing LanguageLinker composition, and seam tests
  status: completed
  evidence: Central reference and skill distinguish automatically embedded defaults/binary rebuild from arbitrary workspace runtime files/index refresh; preserve language-owned operations, exact provider/lexical/package proof and existing delta seams.
- subtask_ref: annotated-framework-manifest-examples
  title: Provide annotated, copy-pasteable reference examples in skills/contextunity-forge/references/frameworks/ for Django, Starlette, FastAPI, Nuxt 3, Vue 3, HTMX, Alpine.js, and GridViewSpec
  status: completed
  evidence: Added bounded exact-bundle-rule Starlette/FastAPI/Nuxt/vue-runtime/HTMX examples; original Django/Alpine/Grid and generic TOML/YAML bytes preserved. example-source-bindings.json records exact selected rule/source hashes. All10 passed strict production writer loader.
- subtask_ref: workspace-extension-and-verification-runbook
  title: Document workspace override mechanisms (.forge/frameworks/, extend vs replace) and verification runbooks using Forge MCP tools (code_map_inspect, code_map_overview) to prove edge resolution
  status: completed
  evidence: docs/runbooks/framework-packs.md plus indexes document overrides/disable/restore, Forge overview/search/inspect/explain and reopened coverage, equal-final cold/delta, separate retained real controls, full R/A/U/E, nonexclusive cohorts, exact attribution and no numeric/performance promise.
- subtask_ref: validate-manifest-examples-test
  title: Add seam test in tests/languages/manifests.rs validating that all reference examples parse cleanly as valid FrameworkManifest instances
  status: completed
  evidence: Accepted existing aggregate function and all negative controls unchanged SHA626f50a8de727319528cf71d8c0cf53030ae64369e8d9386b6065836c949762d. Focused1/0/270filtered; strictClippy0 and full languages271/0/0ignored/0filtered, both exit0. focused.log SHA2aef01d59e369a31bbec9e0a2ec0ebdb7c5f3cae139387201d11d50c72ee869c; registered.log SHAceaffc28a401afe3166fa991459cfd7bdcaf321ba9c409508f5f4986cd3748ec.
receipt:
  commit: ef9333def96f38c773b1a19508252537d3e9c07b
  contract_revision: 4
  passed_at: 2026-10-21T17:04:50.249804464+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 271
      tests_failed: 0
      log: StrictClippy0; languages271passed/0failed/0ignored/0filtered. All10 examples through actual writer loader; exact focused1/0/270filtered. Source freeze 29287b64dc8bf912e62a5c120958c8a225a5afbfadfd519ce51f086eda9a797a unchanged during both gates. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-authoring-guide-build/packet.json SHAad5bff1d41d269f6717d4f4caef580f6fa828901a571a13d7cc3e3d588150f9e; focused log 2aef01d59e369a31bbec9e0a2ec0ebdb7c5f3cae139387201d11d50c72ee869c; registered log ceaffc28a401afe3166fa991459cfd7bdcaf321ba9c409508f5f4986cd3748ec; writer unchanged 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a. All5 advisory subtasks completed via supported API. No new test function/source changes, Cargo released.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All11 admitted product blobs candidate-exact,415 inputs frozen; protected header/other9 tasks/own acceptance exact. Writer unchanged, no engine/data/profile/out-of-scope draft edits. validation.json. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-authoring-guide-review7/packet.md SHA256 106ce66ed1225df252fcf641dde805900293c1e0c6f15dadf0294f09afa6358e.
        claims:
          applicable: true
          evidence: All8 concrete+2generic examples strict writer loader,15 new exact bundle selections; originals/negative tail byte-exact,0new test functions.50 links/frontmatter/fencedTOML/diff checks pass. Independent registered Clippy0+languages271/0 exit0; logSHA f5dd21fcbd93b9d2bddc2147dbdb002814748fff3da1541b2cd9e3c290c24713. No85 claim. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-authoring-guide-review7/packet.md SHA256 106ce66ed1225df252fcf641dde805900293c1e0c6f15dadf0294f09afa6358e.
        concurrency:
          applicable: true
          evidence: No transaction/lock/cache/task-runtime change. Fixed root/HEAD/candidate/rev4/digest/independent claim7;415 hashes before/after unchanged, sole command terminal, Cargo released. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-authoring-guide-review7/packet.md SHA256 106ce66ed1225df252fcf641dde805900293c1e0c6f15dadf0294f09afa6358e.
        project_isolation:
          applicable: true
          evidence: Filename/local scope/package proof, enable tombstone and runtime-vs-embedded boundaries documented against existing owners. Linked overrides remain independent; no install/global activation, source/index/release/main operations. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-authoring-guide-review7/packet.md SHA256 106ce66ed1225df252fcf641dde805900293c1e0c6f15dadf0294f09afa6358e.
        administration:
          applicable: true
          evidence: Distinct accepted builder/reviewer; five subtasks supported-completed, authority/history/profiles/hooks unchanged. Same reviewer supported autoCommit delivery only, no manual receipt or duplicate commit. Later integration85 authority remains coordinator-owned. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-final-framework-data/.forge/evidence/m022-authoring-guide-review7/packet.md SHA256 106ce66ed1225df252fcf641dde805900293c1e0c6f15dadf0294f09afa6358e.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: A fixed in-repository framework fixture measures resolved / (resolved + ambiguous + unresolved), excluding external imports. The candidate must exceed both the measured branch-point baseline and 85%; only coverage counts and percentage are acceptance evidence. Timing, memory, and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - src/db/writer.rs is an unchanged strict production-loader proof dependency only, at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a. No source edits, visibility changes, wrappers or engine behavior are admitted; preserve all existing guide requirements and aggregate example validation.
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

1. `docs/reference/framework-manifests.md` documents filename-stem identity and the actual root fields (`extend`, `replace`, `receivers`, `builtins`, `filters`, `routes`, `class_hierarchy_receivers`, `route_decorators`, `route_calls`, `di_calls`, and `semantic_rules`), plus supported package inputs, activation scope, and workspace overrides (`extend` / `replace`). It does not describe a `[framework]` table.
2. `skills/contextunity-forge/references/frameworks/` contains valid, verified `.toml.example` files for Django, Starlette, FastAPI, Nuxt 3, Vue 3, HTMX, Alpine.js, and GridViewSpec.
3. Seam test in `tests/languages/manifests.rs` verifies that all reference examples parse without error.
4. `cargo test --test languages` passes with strict Clippy.

### task: real-control-residual-semantic-repair

```yaml
task_ref: real-control-residual-semantic-repair
target: Resolve the five observed real-control residual clusters only where explicit static or configured scope evidence proves a unique target, preserving uncertainty and all accepted framework semantics
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- typed-framework-semantic-linking
scope:
- src/engine/languages/typescript/value_flow.rs
- src/engine/languages/python/projections.rs
- src/engine/languages/html.rs
- src/engine/languages/manifests/projections.rs
- src/engine/languages/manifests/syntax.rs
- src/engine/languages/dependency_registry.rs
- src/engine/linker/named_bindings.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker/projections.rs
- src/engine/linker/html_templates.rs
- src/engine/linker/resolution.rs
- src/engine/linker/routes.rs
- src/engine/linker/framework_relations.rs
- src/engine/linker/contracts.rs
- src/engine/linker.rs
- src/db/delta.rs
- src/core/semantic.rs
- src/core/typed_facts.rs
- tests/languages/manifests.rs
- tests/languages/python/frameworks.rs
- tests/languages/typescript/frameworks.rs
- tests/languages/html/templates.rs
- docs/reference/framework-manifests.md
- docs/reference/framework-manifests-gridviewspec.md
- docs/reference/framework-manifests-nuxt-vue-pinia.md
- skills/contextunity-forge/references/framework-packs.md
- skills/contextunity-forge/references/frameworks/declarative-rules.toml.example
- skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example
- src/db/writer.rs
invariants:
- This additive generic follow-up preserves all ten completed TaskSpec and receipt literals and every accepted revision 4, 6, 7, 8, 9 and 10 capability and proof history. It binds the current user-approved real-control condition without reopening or bulk-synchronizing historical tasks. No framework spelling branch, new root operation, module or application graph, CFG, synthetic Node, new persisted fact vector, parallel linker/cache/index, source-recovery pass or runtime evaluation is admitted.
- Configured setup-return eligibility is separate from ordinary ReturnSummary policy and accumulated in the same existing traversal. Permit non-returning if/try/catch/finally initialization only when the sole active return is the final direct literal object. Preserve async/generator, alternate/bare/multiple return, abrupt exit, loop/switch/ternary, trailing-statement, duplicate/spread/computed-key, non-node state, lexical and callable-write guards. Conservatively reject unsupported destructuring writes and deletion rather than infer their effects. Nested callable exits belong to their own scope. No ordinary return-summary widening, branch-return merge, CFG or second body walk is admitted.
- Extend existing named_binding call_reference with optional bounded strict provider_scopes descriptors containing workspace, project and package. Absence preserves local-project behavior. Resolve only already enabled adapter identities and already collected nearest project/own-package identities, once during preparation; no new discovery or filesystem lookup. Query local plus explicitly admitted provider scopes as a complete set, preserving unknown poisoning and distinct-target ambiguity. Installation-specific workspace aliases and project paths belong only to explicit local TOML, never bundled assumptions or Rust spellings. No global domain/key join, source-order winner or builtin fallback is admitted.
- Extend existing named_binding with AssignmentDefinition carrying a strict imported receiver_type module/export, member and normalized provider-relative template_root, and TemplateReference restricted to the existing filter symbol kind. Project existing HTML filter references without another parse or AST walk. Python literal subscript assignment emits the closed NamedBindingRole RegistryDefinition with required receiver and target in the existing vector; legacy Definition serialization is unchanged. Prove receiver type using existing canonical imported-type/value-flow resolution and preserve rebinding, lexical, dynamic key/RHS, target uniqueness and uncertainty. Resolve filters only within the declared template root, nearest manifest/workspace and effective domain. Do not infer environment-helper flow, add global same-name joins or relax ordinary template-loader return policy.
- Extend HttpClientCall with optional target_context containing bounded nonempty request_owners and route_owners lists of normalized confined canonical paths, including only explicit existing linked-workspace qualifiers through the existing workspace_path contract. Prepare membership and route selection once from effective origin. Absent context or a request outside request_owners preserves current matching; an applicable context restricts matching to its declared route owners, with no global fallback when no provider or method/path match exists. Distinct matching route identities remain ambiguous. Context is an explicit configuration assertion, independently checked against the real client/app registration source chain; the engine does not infer that chain or detect a semantically incorrect but valid user assertion. EndpointProjection, named routes and HTML requests retain their existing behavior.
- Preserve all fourteen observed runtime-object-field HTML requests as unresolved with unchanged raw identities and no endpoint edges. They are not literal or finite constant values; route-shape similarity never proves their target. The sixty callable, fourteen renderer, seven filter and two Python request inventories are source-bound candidates, not mandatory resolved-count thresholds. Only one uniquely proven indexed target produces an exact edge; E is not R, and missing, dynamic, shadowed or duplicate identities retain U/A without guessed edges.
- Every changed fact role, receiver/target reference key, effective scoped policy and provider-only mutation participates in existing strict codec validation, canonical resolution contracts, effective-rules/build fingerprint and selective delta. Reuse old/new reference keys, scope-filtered shared owners and attempted-set once-path canonical hydration for missing-provider add/removal, target-only edits, rule/descriptor/root/context changes and unchanged-consumer replay. Cold and delta must agree without source reads, global relink, duplicate full-facts maps or a new reverse index. Existing engine/core source fingerprint supplies extraction invalidation; no new cache mechanism is admitted.
- All milestone performance guardrails remain binding. Keep the single iterative AST traversal and completed summaries, bounded immutable prepared descriptors and existing reducer/dispatch limits, lean canonical compressed facts, existing staging/publication and deterministic commitments. No per-reference FS/SQL/JSON/TOML/source work, whole-body rescans, arbitrary numerical performance promise or benchmark is admitted.
- Extend only existing production-seam test functions with observed control snippets and existing negative phases; no new test function, test file, synthetic corpus or per-defect test flood. Keep proof proportional to actual changed mechanisms and reuse precision negatives once. Returns-only changes require no invented DTO-boundary suite; new closed forms retain strict validation and legacy compatibility. Production writer, reopened SQLite, exact canonical targets and targeted cached-provider/configuration cold-delta proof remain required.
- Reuse the final retained Commerce baseline database SHA-256 507e549bb31273c65ec3e867b427439b2b59f34bd5a86f633304d50705fdc954 and Library baseline SHA-256 ee0c61aa987bfa9cdd19a5a375116568d093c5ec3431cc3f178638265304a617 with their frozen source/configuration/binary identities. Use isolated copies of the same controls, disclosing only admitted local TOML policy changes. Freeze one combined candidate, build it once and index each control once; no baseline rebuild or reindex. Report every cohort row, complete per-control/per-language/fixed-framework R/A/U/E, exact gained and preserved edge identities, configuration effects and residuals on that same candidate. No percentage threshold or all-candidates-resolved promise is admitted.
- src/db/writer.rs is an unchanged strict production-seam proof dependency only at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a; no edits, visibility changes or wrappers are admitted in that file. Documentation/examples co-evolve only for the admitted operation extensions and precision boundaries; live controls, unrelated source and historical task authority remain untouched.
subtasks:
- subtask_ref: configured-setup-final-callable-summary
  title: Recover only proven final callable setup members after non-returning initialization using existing traversal and guards
  status: completed
  evidence: Frozen source; strict Clippy + complete languages target271/0 and affected targets134/0. Real60 inventory49R/11U; all49 canonical calls verified, remaining11 blocked by explicit unbound ~/stores/messages import. E/returns-final-analysis/report.md SHA03eb0634dc1cf85fc40084c3e4e514a341b589f01c8997ecfcd81d1f2a6f4e29; no ordinary-summary widening.
- subtask_ref: scoped-grid-renderer-associations
  title: Join explicit local and linked provider scopes to unique registered renderer targets without global-name fallback
  status: completed
  evidence: 'Final SQL comparison005 PASS: all14 renderer identities R with expected indexed registered callbacks under explicit provider scopes; no precision/preservation failures among97 source-bound rows. Summary SHA a3d57e493144a03ce295e85d47e01a672e2deaf9a13ef99a8c423919e07fba1d. Provider deletion/restoration and scope-negative cold/delta tests passed. Build strict Clippy/languages271+affected134 PASS; independent Review runtime repeated strict Clippy and all405 affected tests PASS with snapshot unchanged, log SHA fb53c802196bcd6caf4aba97d3a78648120c03f41ac151154d177d03ff388e6a.'
- subtask_ref: scoped-local-template-filter-registrations
  title: Link imported-type-proven local filter registrations only to their configured template roots
  status: completed
  evidence: Final SQL comparison005 PASS:97 source-bound rows, zero precision/preservation failures; summary SHA a3d57e493144a03ce295e85d47e01a672e2deaf9a13ef99a8c423919e07fba1d. All7 frozen filter uses U→R, plus13 additional real uses; all20 resolve to exact registered imported local functions. E/filters-final-real-attribution.json SHA473220e430f37d5c09d305fa8fa7df8c92afd38a9912bb74515763e0cb811562. Runtime replay002 seven positives/13 negative cases/provider and rule deletion-restoration cold-delta parity PASS; complete languages271/0 plus affected134/0, strict ClippyPASS. Same-line distinct filters independently adjudicated; reporting helper source registration mapping does not filter by row expected-target oracle.
- subtask_ref: preserve-dynamic-html-request-precision
  title: Preserve all fourteen real dynamic HTML request identities as unresolved with no guessed endpoint edge
  status: completed
  evidence: All14 frozen dynamic HTML identities retain U and no endpoint edge, exact multiplicity preserved; E/comparison-results-003/every-cohort-row.json HTML cohort. No guessed request targets or runtime evaluation introduced.
- subtask_ref: scope-python-test-client-route-requests
  title: Apply explicit request-owner and route-owner configuration to the two source-adjudicated Python requests while preserving other scopes and ambiguity
  status: completed
  evidence: Both2 real source-adjudicated Python request identities nowR to exact route handlers via explicit configured owner scopes. E/route-diagnostic-replay-002/verification.json SHAd2e075120d3cc658a4bcf2962ff2ef62d48f466d7a7e0850af90f63df849524d:58/58 assertions,15 successfulCLI invocations, cold/delta parity including provider/config removal/restoration and outside ambiguity. Complete languages271/0 and additional134/0.
status: completed
receipt:
  commit: ced8ce6e773ed79db97c71bd82b14e0600839c2b
  contract_revision: 1
  passed_at: 2026-10-21T20:05:52.864609499+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 271
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All32 changed files within live admitted scope/extensions;415 inputs exact; protected header and ten historical TaskSpec/receipt literals exact prefix; writer9a5684 unchanged. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-real-residual-repair/formal-review-004.md SHA256 9c185f7d66f9d0fc2e3809be79b2b929dc7f96acdf646f2e4da35c4803bdfb04; binding SHA256 10e89a64ec9b73d7c00519d40cb612209d4b115e83421a78a1b91027cc2a1ffe.
        claims:
          applicable: true
          evidence: Cumulative full source/docs/TOML audit and boundary closures; final strict Clippy0 + complete affected405/0. SQL00597 rows0 failures72R25U, exact cohort targets/handlers,20 real filter targets, all86 gained edges adjudicated, zero baseline edge/identity/R loss;11 unbound import U+14HTML U/noedge explicit. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-real-residual-repair/formal-review-004.md SHA256 9c185f7d66f9d0fc2e3809be79b2b929dc7f96acdf646f2e4da35c4803bdfb04; binding SHA256 10e89a64ec9b73d7c00519d40cb612209d4b115e83421a78a1b91027cc2a1ffe.
        concurrency:
          applicable: true
          evidence: No lifecycle transaction/lock/publication change or new cache; same rev1/claim4/snapshot5555322,415 hashes unchanged before/after review gate, selective cached provider/config delta parity. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-real-residual-repair/formal-review-004.md SHA256 9c185f7d66f9d0fc2e3809be79b2b929dc7f96acdf646f2e4da35c4803bdfb04; binding SHA256 10e89a64ec9b73d7c00519d40cb612209d4b115e83421a78a1b91027cc2a1ffe.
        project_isolation:
          applicable: true
          evidence: Prepared enabled provider scopes and nearest project/own-package identities; confined request/route owners and provider-relative template roots; canonical module+export proof; unknown/duplicate/missing failclosed. Four local policies are explicit isolated assertions, no global join/fallback or app inference. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-real-residual-repair/formal-review-004.md SHA256 9c185f7d66f9d0fc2e3809be79b2b929dc7f96acdf646f2e4da35c4803bdfb04; binding SHA256 10e89a64ec9b73d7c00519d40cb612209d4b115e83421a78a1b91027cc2a1ffe.
        administration:
          applicable: true
          evidence: Distinct accepted builder/reviewer, five supported-completed subtasks, supported scope extensions, unchanged profiles/hooks/history. No manual receipt/commit/main/publication/store mutation. Current source immutable; supported same-snapshot autoCommit delivery only. Packet /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-real-residual-repair/formal-review-004.md SHA256 9c185f7d66f9d0fc2e3809be79b2b929dc7f96acdf646f2e4da35c4803bdfb04; binding SHA256 10e89a64ec9b73d7c00519d40cb612209d4b115e83421a78a1b91027cc2a1ffe.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: Verify real Commerce and Library controls separately against frozen source, configuration, binary and database baseline identities. Report complete R/A/U/E counts and same-cohort per-language and framework comparisons; external classification is not exact resolution. Prove exact-edge gains without losing correct baseline exact edges, preserve intentional unresolved/ambiguous outcomes and negative precision controls, and disclose all changes, configuration effects and residual limitations. No percentage threshold or synthetic experiment is required. Timing, memory and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - This additive generic follow-up preserves all ten completed TaskSpec and receipt literals and every accepted revision 4, 6, 7, 8, 9 and 10 capability and proof history. It binds the current user-approved real-control condition without reopening or bulk-synchronizing historical tasks. No framework spelling branch, new root operation, module or application graph, CFG, synthetic Node, new persisted fact vector, parallel linker/cache/index, source-recovery pass or runtime evaluation is admitted.
    - Configured setup-return eligibility is separate from ordinary ReturnSummary policy and accumulated in the same existing traversal. Permit non-returning if/try/catch/finally initialization only when the sole active return is the final direct literal object. Preserve async/generator, alternate/bare/multiple return, abrupt exit, loop/switch/ternary, trailing-statement, duplicate/spread/computed-key, non-node state, lexical and callable-write guards. Conservatively reject unsupported destructuring writes and deletion rather than infer their effects. Nested callable exits belong to their own scope. No ordinary return-summary widening, branch-return merge, CFG or second body walk is admitted.
    - Extend existing named_binding call_reference with optional bounded strict provider_scopes descriptors containing workspace, project and package. Absence preserves local-project behavior. Resolve only already enabled adapter identities and already collected nearest project/own-package identities, once during preparation; no new discovery or filesystem lookup. Query local plus explicitly admitted provider scopes as a complete set, preserving unknown poisoning and distinct-target ambiguity. Installation-specific workspace aliases and project paths belong only to explicit local TOML, never bundled assumptions or Rust spellings. No global domain/key join, source-order winner or builtin fallback is admitted.
    - Extend existing named_binding with AssignmentDefinition carrying a strict imported receiver_type module/export, member and normalized provider-relative template_root, and TemplateReference restricted to the existing filter symbol kind. Project existing HTML filter references without another parse or AST walk. Python literal subscript assignment emits the closed NamedBindingRole RegistryDefinition with required receiver and target in the existing vector; legacy Definition serialization is unchanged. Prove receiver type using existing canonical imported-type/value-flow resolution and preserve rebinding, lexical, dynamic key/RHS, target uniqueness and uncertainty. Resolve filters only within the declared template root, nearest manifest/workspace and effective domain. Do not infer environment-helper flow, add global same-name joins or relax ordinary template-loader return policy.
    - Extend HttpClientCall with optional target_context containing bounded nonempty request_owners and route_owners lists of normalized confined canonical paths, including only explicit existing linked-workspace qualifiers through the existing workspace_path contract. Prepare membership and route selection once from effective origin. Absent context or a request outside request_owners preserves current matching; an applicable context restricts matching to its declared route owners, with no global fallback when no provider or method/path match exists. Distinct matching route identities remain ambiguous. Context is an explicit configuration assertion, independently checked against the real client/app registration source chain; the engine does not infer that chain or detect a semantically incorrect but valid user assertion. EndpointProjection, named routes and HTML requests retain their existing behavior.
    - Preserve all fourteen observed runtime-object-field HTML requests as unresolved with unchanged raw identities and no endpoint edges. They are not literal or finite constant values; route-shape similarity never proves their target. The sixty callable, fourteen renderer, seven filter and two Python request inventories are source-bound candidates, not mandatory resolved-count thresholds. Only one uniquely proven indexed target produces an exact edge; E is not R, and missing, dynamic, shadowed or duplicate identities retain U/A without guessed edges.
    - Every changed fact role, receiver/target reference key, effective scoped policy and provider-only mutation participates in existing strict codec validation, canonical resolution contracts, effective-rules/build fingerprint and selective delta. Reuse old/new reference keys, scope-filtered shared owners and attempted-set once-path canonical hydration for missing-provider add/removal, target-only edits, rule/descriptor/root/context changes and unchanged-consumer replay. Cold and delta must agree without source reads, global relink, duplicate full-facts maps or a new reverse index. Existing engine/core source fingerprint supplies extraction invalidation; no new cache mechanism is admitted.
    - All milestone performance guardrails remain binding. Keep the single iterative AST traversal and completed summaries, bounded immutable prepared descriptors and existing reducer/dispatch limits, lean canonical compressed facts, existing staging/publication and deterministic commitments. No per-reference FS/SQL/JSON/TOML/source work, whole-body rescans, arbitrary numerical performance promise or benchmark is admitted.
    - Extend only existing production-seam test functions with observed control snippets and existing negative phases; no new test function, test file, synthetic corpus or per-defect test flood. Keep proof proportional to actual changed mechanisms and reuse precision negatives once. Returns-only changes require no invented DTO-boundary suite; new closed forms retain strict validation and legacy compatibility. Production writer, reopened SQLite, exact canonical targets and targeted cached-provider/configuration cold-delta proof remain required.
    - Reuse the final retained Commerce baseline database SHA-256 507e549bb31273c65ec3e867b427439b2b59f34bd5a86f633304d50705fdc954 and Library baseline SHA-256 ee0c61aa987bfa9cdd19a5a375116568d093c5ec3431cc3f178638265304a617 with their frozen source/configuration/binary identities. Use isolated copies of the same controls, disclosing only admitted local TOML policy changes. Freeze one combined candidate, build it once and index each control once; no baseline rebuild or reindex. Report every cohort row, complete per-control/per-language/fixed-framework R/A/U/E, exact gained and preserved edge identities, configuration effects and residuals on that same candidate. No percentage threshold or all-candidates-resolved promise is admitted.
    - src/db/writer.rs is an unchanged strict production-seam proof dependency only at SHA-256 9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a; no edits, visibility changes or wrappers are admitted in that file. Documentation/examples co-evolve only for the admitted operation extensions and precision boundaries; live controls, unrelated source and historical task authority remain untouched.
    architectural_notes:
    - 'decisions: Anti-loop diagnostic approach produced observed boundaries before authorizing repair. Fresh diagnostic binary SHA1417efebfab094b11dea9d11f6c3c8922f858dd24a8c7d4a073c8dc358075d49, diagnostic-binary001 exit0. Real Commerce diagnostic build exit0/frozen (diagnostic-commerce001 logSHA131f8227f6029b3cc57eb7514bde840c97836592c4e9d977e78c36a998f57771), all7Jinja U uncertainregistration. filter-diagnostic-real-boundary.json proves all8 producer+codec definitions and exact callback importtargets, but prepared receiver compares external module jinja2 to jinja2.Environment. Repair must prove BOTH canonical module+export, including same-module/same-import-line wrong-export rejection. Existing fixture separately had site.filters external instead of canonical .filters; only3fixture import literals corrected, expectedtargets/oracles unchanged. Persistent route-diagnostic-case phases prove outside.py remains staleA after local route deletion; freshcold and forced consumer-only delta resolve same linkedGET. Shared key GET /test not seeded: deleted/new route names deliberately omitted in generic old/new dirty-name loops. Authorized minimal existing route.name dirty-key inclusion and canonical receiver identity fix, no newmechanism/ASTpass, no third blindfullsuite loop. First revalidate directCLI persistent cases on freshcombinedbinary, then finaltaskgate and actual97-row SQL.'
    - 'deferred: Actual retained residual, not a claimed repair: 11/60 returned-member cohort identities remain U, all ui/tests/unit/agui-streaming.test.ts. Its explicit ~/stores/messages import is E (''no indexed provider for ~.stores.messages''), hence no proven import/factory association; three provider setup facts now populated correctly. Configured-name fallback intentionally cannot override explicit unbound imports. E/returns-final-analysis/report.md and analysis.json source-bind all11. Current admitted task permits source-bound candidate residuals and excludes module/config graph expansion; no alias semantics inferred from ui/tsconfig.json extending generated .nuxt/tsconfig.json. Fourteen dynamic HTML requests also intentionally remain U/no endpoint edge. No new alias-resolution feature admitted or implemented.'
    - 'decisions: Coordinator reserved Deliver claim5 under worker codex-sol-real-residual-deliverer before reading live same-reviewer-session guidance. Normal reclaim to codex-sol-real-residual-reviewer is blocked TASK_ALREADY_CLAIMED. Preserve accepted stages and active lease; no reset/reopen/rejection/store edit. Hand off execution of the RESERVED active claim5 and its exact bound template to the SAME independent reviewer session /root/m022_generic_sol_reviewer, which accepted Review4 and authored no candidate source/tests. That session performs delivery as the reserved delivery worker, still distinct from accepted builder codex-sol-real-residual-builder. No new review or code changes; accepted snapshot55553220e22db267a3abe5d0f05f2402023fa748 and proofs remain unchanged. This is coordinator-to-reviewer execution ownership handoff, not a change to task authority or evidence.'
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

1. The frozen sixty-callable, fourteen-renderer, seven-filter, fourteen-dynamic-HTML and two-Python-request ledgers in `.forge/evidence/m022-real-residual-repair/` bind every original identity, status, source hash and candidate target or retained uncertainty. The implementation addresses all five contours in one combined candidate; no count is a blanket exact-resolution requirement.
2. Existing shared manifest, configured-result, GridViewSpec and template seams use actual observed source shapes and prove only the admitted changes: unique canonical targets, explicit scope causality, preserved ordinary behavior and uncertainty, and targeted provider/rule/configuration transitions with reopened facts/coverage and equal-final cold/delta commitments. No new test functions or synthetic fixture corpus are added.
3. Strict schema validation, closed-role canonical persistence/reference keys and compatibility, once-prepared scope selection and existing cache invalidation cover the changed mechanisms. Missing/disabled/unrelated provider scopes, dynamic or rebound receiver/target, duplicate definitions, template-root boundaries and scoped missing route owners never create guessed exact edges. Explicit policy assertions are distinguished from inferred source facts.
4. One stable candidate passes strict Clippy and the affected existing languages target. One candidate measurement per retained real control reports all cohort outcomes, complete R/A/U/E tables, exact-edge gains and no loss of correct baseline exact edges, with all fourteen dynamic HTML requests retained U, explicit local-configuration effects and remaining U/A disclosed. Baseline databases and live original repositories remain unchanged.

### task: final-integration-scope-verification

```yaml
task_ref: final-integration-scope-verification
target: Verify existing final parser/framework integration, documentation and Cargo caller proof within the complete remaining handoff scope without changing implementation
proof_policy: direct-proof
contract_revision: 1
scope:
- README.md
- docs/adr/0013-data-driven-framework-manifests.md
- docs/adr/0017-serde-framework-manifest-formats.md
- src/AGENTS.md
- src/core/models.rs
- src/db/ingest.rs
- src/engine/ast/mod.rs
- src/engine/languages/html/htmx.rs
- src/engine/languages/html/javascript.rs
- src/engine/languages/manifests.rs
- src/engine/languages/manifests/fastmcp.toml
- src/engine/languages/manifests/h3.toml
- src/engine/languages/manifests/jinja2.toml
- src/engine/languages/manifests/nestjs.toml
- src/engine/languages/manifests/psycopg.toml
- src/engine/languages/manifests/semantic.rs
- src/engine/languages/manifests/web.toml
- src/engine/languages/mod.rs
- src/engine/languages/python.rs
- src/engine/languages/python/dependencies.rs
- src/engine/languages/python/fastmcp.rs
- src/engine/languages/python/module_exports.rs
- src/engine/languages/python/render_context.rs
- src/engine/languages/python/template_loaders.rs
- src/engine/languages/python/value_flow.rs
- src/engine/languages/registries/javascript_packages.rs
- src/engine/languages/semantic.rs
- src/engine/languages/typescript/dom.rs
- src/engine/languages/typescript/mod.rs
- src/engine/languages/vue.rs
- src/engine/languages/vue/setup_facts.rs
- src/engine/languages/vue/template.rs
- src/engine/linker/html_scripts.rs
- tests/acdd/tasks.rs
- tests/incremental/scope.rs
- tests/languages/profiles/legacy_hooks.rs
- src/db/writer.rs
- src/engine/languages/typescript/frameworks.rs
- tests/languages/typescript/frameworks.rs
depends_on:
- real-control-residual-semantic-repair
invariants:
- Verification only. Every tracked implementation, test, configuration, skill and documentation byte outside this managed milestone file remains identical to ced8ce6e773ed79db97c71bd82b14e0600839c2b. No production, test or documentation edits, new requirements, framework spellings, operations, parser passes, persisted forms or runtime mechanisms are admitted.
- The 36 remaining paths are enumerated in .forge/evidence/m022-handoff-final/before-scope.json. Thirty-four are covered by historically delivered scoped snapshots from 21 completed task revisions, recorded in before-historical-scopes.json. README.md and tests/acdd/tasks.rs are the independently reviewed existing coordinator documentation and Cargo caller-oracle adjustments. This task verifies their existing current content under the approved milestone contract; it never rewrites historical scope or acceptance.
- Preserve the complete eleven-task prefix and every historical TaskSpec, receipt, digest, revision, gate, claim and proof. Use task create for this selected task only; no bulk sync, reset, reopen, manual receipt edit, profile change or main integration is admitted.
- Use the existing vue_destructured_composable_call_uses_indexed_return_member production seam and unchanged writer dependency. Direct existing-code proof is sufficient; do not invent a failing assertion, synthetic test or new fixture. Build verifies strict Clippy and the complete affected languages, acdd and incremental targets; current source-bound real-control SQL and final coverage evidence remain applicable unchanged.
- Preserve all milestone correctness and performance invariants and retained control identities. Run the required final milestone gate and independent combined review after delivery on the exact resulting content and selected target base. No baseline rebuild, control reindex, benchmark or new coverage measurement is admitted.
status: completed
receipt:
  commit: d4a27b805b5e5420e45967ffa14c0631c6ad35c9
  contract_revision: 1
  passed_at: 2026-10-22T00:13:24.281078096+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages --test acdd --test incremental
      exit_code: 0
      tests_passed: 404
      tests_failed: 0
      log: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages --test acdd --test incremental", "exit_code": 0, "tests_passed": 404, "tests_failed": 0, "targets": {"acdd": 103, "incremental": 30, "languages": 271}, "contract_seam": {"command": "cargo test --test languages vue_destructured_composable_call_uses_indexed_return_member", "exit_code": 0, "tests_passed": 1, "tests_failed": 0}, "log_sha256": {"contract-seam.log": "7fd0301a78c67a85d8aee38f64cae64e82ced1b180f43ba9b7abaf36263126ae", "build-clippy.log": "cd26425abfd34e161302da7f4343751b2ffbddf5b7d0edb628973da95c8fd23d", "build-targets.log": "0454612a0550122da551564810dcd99a6b005872f47cd2c08342b3930fc0c521", "source-binding.json": "039f95ab9a964f96be37e53ce025fe96914e8916de182c01f0db8ea9f2e0767a", "source-binding-after-build.json": "6e654de6f0252a0d7e8d484a1dc4ef757e38f23130df10bfbb0625e6ec0ac5bd"}, "unchanged_proof_dependencies": {"src/core/commitments.rs": "e14ad8c8e257eb0a1f68ad109d9669e8e0668f54fe1a5e59ab909b8ee466ba7b", "src/db/writer.rs": "9a5684b13be63bdc551c63e984640d46559f1a3113c9633df0c4d58326bf2f2a"}, "scoped_commands_note": "Both claim and task inspect omitted scoped_commands; used registered rust-check-and-test prefix with all three admitted complete targets."}'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'All 40 scope files match candidate050f5d8f and prior accepted content; all414 nonmanaged inputs unchanged; old198063-byte milestone prefix exact. Report /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/report.md SHA256 a57ee968fd434a2b09d4cd2804aa0105f68de7652deef5e385121cdfbc7aab02; binding /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/binding.json SHA256 118deabfb62792cf93be83a52f2f5af7878dae2e7915a244f4cd4ccbe9b7f2ea. '
        claims:
          applicable: true
          evidence: 'Verification-only direct-proof contract satisfied by existing Vue production seam, configured positives/negatives and unchanged realSQL005 proof. No implementation/test/docs changes or new requirements. Report /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/report.md SHA256 a57ee968fd434a2b09d4cd2804aa0105f68de7652deef5e385121cdfbc7aab02; binding /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/binding.json SHA256 118deabfb62792cf93be83a52f2f5af7878dae2e7915a244f4cd4ccbe9b7f2ea. '
        concurrency:
          applicable: true
          evidence: 'Current readback of all11 old TaskSpecs/digests/revisions/gates/claims/receipts/snapshots equals pre-admission records. No reopen/reset/bulk sync; reviewer distinct from builder. Report /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/report.md SHA256 a57ee968fd434a2b09d4cd2804aa0105f68de7652deef5e385121cdfbc7aab02; binding /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/binding.json SHA256 118deabfb62792cf93be83a52f2f5af7878dae2e7915a244f4cd4ccbe9b7f2ea. '
        project_isolation:
          applicable: true
          evidence: 'Explicit milestone root and scoped candidate verified; source/configuration/workspace behavior unchanged. Retained real-control SQL fences and precision residuals preserved. Report /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/report.md SHA256 a57ee968fd434a2b09d4cd2804aa0105f68de7652deef5e385121cdfbc7aab02; binding /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/binding.json SHA256 118deabfb62792cf93be83a52f2f5af7878dae2e7915a244f4cd4ccbe9b7f2ea. '
        administration:
          applicable: true
          evidence: 'Accepted Contract/Build rev1; strictClippy0 and complete languages271+acdd103+incremental30=404passed/0failed; focused1 not doublecounted. Same independent reviewer/session will Deliver unchanged candidate via autoCommit. Report /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/report.md SHA256 a57ee968fd434a2b09d4cd2804aa0105f68de7652deef5e385121cdfbc7aab02; binding /home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/022-declarative-framework-manifests-and-ecosystem-packs/.forge/evidence/m022-handoff-final/final-scope-task/review/binding.json SHA256 118deabfb62792cf93be83a52f2f5af7878dae2e7915a244f4cd4ccbe9b7f2ea. '
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-FRAMEWORK-ACTIVATION: Bundled rules use declared-package activation where applicable; workspace-local .forge/frameworks/<filename-stem> manifests remain workspace scoped. Template and frontend relations without package metadata require explicit extractor facts and typed rules with empty activation; no unsupported marker-activation syntax is implied.'
    - 'INV-DECLARATIVE-LINKING: Framework API identities, receiver families, decorator and keyword names, directory conventions, activation conditions, and mappings to graph relations are validated manifest data. Language extractors own grammar, lexical scope, imports, and finite typed operations; extractors, validators, RuleIR compilation, caches, and linkers never select framework behavior through hardcoded framework, package, namespace, or API spellings. Static bundle catalogues only bind pack identities to data files. Composition uses the existing LanguageLinker seam; arbitrary extraction code and parallel linkers are forbidden.'
    - 'INV-PACK-ENGINE-SEPARATION: Framework-pack tasks change manifest data, domain tests, and affected documentation. Production Rust and typed operation changes belong to the admitted generic prerequisite task; extending file scope alone never admits new engine semantics.'
    - 'INV-MANIFEST-CAUSALITY: Removing, replacing, or deactivating an effective rule removes its framework semantic facts and relations while preserving ordinary language calls and imports. Renaming API, attribute, keyword, and directory spellings in both rule and fixture preserves equivalent semantics without production code changes.'
    - 'INV-SEMANTIC-FACTS-AND-DELTA: Parsing emits typed semantic facts with rule provenance. Linking and delta consume those facts without rereading source or framework-specific fallbacks; cold and delta builds agree after effective rule, activation, and source changes. Dynamic, shadowed, missing, or ambiguous identities never produce guessed exact edges.'
    - 'INV-PRECISION-AND-ZERO-FALSE-EDGES: Every new framework relation binds only to one unique, statically verified target. Missing targets produce unresolved coverage, duplicate targets produce ambiguous coverage, and neither case emits an edge.'
    - 'INV-FRAMEWORK-COVERAGE: Verify real Commerce and Library controls separately against frozen source, configuration, binary and database baseline identities. Report complete R/A/U/E counts and same-cohort per-language and framework comparisons; external classification is not exact resolution. Prove exact-edge gains without losing correct baseline exact edges, preserve intentional unresolved/ambiguous outcomes and negative precision controls, and disclose all changes, configuration effects and residual limitations. No percentage threshold or synthetic experiment is required. Timing, memory and throughput are not gates.'
    - 'INV-FRAMEWORK-AUTHORING-GUIDE: Reference documentation and skill examples describe the filename-derived manifest identity, the actual typed schema, supported activation behavior, exact-edge requirements, and the existing extraction and linking seams.'
    - Verification only. Every tracked implementation, test, configuration, skill and documentation byte outside this managed milestone file remains identical to ced8ce6e773ed79db97c71bd82b14e0600839c2b. No production, test or documentation edits, new requirements, framework spellings, operations, parser passes, persisted forms or runtime mechanisms are admitted.
    - The 36 remaining paths are enumerated in .forge/evidence/m022-handoff-final/before-scope.json. Thirty-four are covered by historically delivered scoped snapshots from 21 completed task revisions, recorded in before-historical-scopes.json. README.md and tests/acdd/tasks.rs are the independently reviewed existing coordinator documentation and Cargo caller-oracle adjustments. This task verifies their existing current content under the approved milestone contract; it never rewrites historical scope or acceptance.
    - Preserve the complete eleven-task prefix and every historical TaskSpec, receipt, digest, revision, gate, claim and proof. Use task create for this selected task only; no bulk sync, reset, reopen, manual receipt edit, profile change or main integration is admitted.
    - Use the existing vue_destructured_composable_call_uses_indexed_return_member production seam and unchanged writer dependency. Direct existing-code proof is sufficient; do not invent a failing assertion, synthetic test or new fixture. Build verifies strict Clippy and the complete affected languages, acdd and incremental targets; current source-bound real-control SQL and final coverage evidence remain applicable unchanged.
    - Preserve all milestone correctness and performance invariants and retained control identities. Run the required final milestone gate and independent combined review after delivery on the exact resulting content and selected target base. No baseline rebuild, control reindex, benchmark or new coverage measurement is admitted.
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

1. The existing production seam and affected targets pass on unchanged implementation bytes. Source-bound final coverage and exact-edge preservation remain unchanged.
2. Independent review verifies this task's existing content, immutable eleven-task history and complete remaining scope. Delivery changes only this managed milestone document.
3. Normal handoff validates the complete current scope and fresh final evidence without bypassing authority, scope, proof or independence guards.
