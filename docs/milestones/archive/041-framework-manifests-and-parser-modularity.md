---
id: m-framework-manifests-and-parser-modularity
title: Data-driven framework manifests on the LanguageLinker seam
doc_type: contract
status: completed
depends_on:
- m-architecture-and-modularity:completed
- m-language-semantics-and-resolution-coverage:completed
owners:
- src/engine/languages/
- src/engine/linker/
- tests/
invariants:
- 'INV-LANGUAGE-LINKER: Framework manifests feed existing LanguageLinker implementations. They do not add a second linker.'
- 'INV-GRAMMAR-BOUNDARY: Universal grammar, lexical scope, and imports stay in the language profile. Vue remains a language profile.'
- 'INV-DECLARED-ACTIVATION: A framework manifest applies only when DependencyRegistry reports that the project declares the framework.'
- 'INV-EQUIVALENCE: A migration preserves resolution-status multisets and edge triples (src, dst, kind). commitment_integrity stays deterministic. Evidence wording may change.'
related_plans: []
started_at: 2026-10-07T14:55:19+00:00
handoff:
  completed_at: 2026-10-07T19:19:53.564543026+00:00
  duration: 4h 24m
  commit: bd1dd36bd15572c368e52d8ef8c11aa057239c8f
  verification:
    command: cargo test --all-targets
    status: passed
    tests_passed: 629
    tests_failed: 0
---

# Data-driven framework manifests on the LanguageLinker seam

## Outcome and purpose

Milestone 040 extracted `LanguageLinker` so each language owns its import and receiver rules. This milestone continues that split. Framework receivers, builtins, template filters, and route patterns that are not universal grammar move into manifests those language linkers already consume.

Milestones 020 and 040 are completed prerequisites. The first accepted task claim activates this planned milestone and records `started_at`. Standard manifests are static tables. User files at `.forge/frameworks/` may use `.toml`, `.yaml`, or `.yml`; serde-backed parsing validates them before publication. Proof is a public writer and SQLite seam, not `direct-proof`.

## Manifest lifecycle and language adapters

Bundled tables live in `src/engine/languages/manifests/*.toml` and are embedded in the Forge binary with `include_str!`. The dependency registry parses them into typed manifests when indexing. A new bundled table needs a source TOML file and an entry in `load_standard_framework_manifests`, followed by a new binary build.

Project adapters live at `<workspace>/.forge/frameworks/<framework-id>.toml`, `.yaml`, or `.yml`. Forge reads them at index-build/update time, takes the identifier from the file stem, validates the required `receivers`, `builtins`, `filters`, and `routes` arrays, and scopes activation to a dependency declaration in that workspace. A project file overrides a bundled table with the same identifier; rules are not merged. The existing language linker must request the identifier and understand the supplied rules. A manifest cannot add a language grammar or dynamically register a new language; a new language needs a compiled `LanguageProfile`, a `lang-*` feature and parser dependency as needed, generated profile registration, and a custom `LanguageLinker` when its resolution requires one. The [framework manifest reference](../../reference/framework-manifests.md) documents formats, examples, failure behavior, and extension steps.

## Tasks in this milestone

### task: framework-manifest-schema-and-loader

```yaml
task_ref: framework-manifest-schema-and-loader
target: Load typed TOML and YAML framework manifests of receivers, builtins, filters, and routes into the existing LanguageLinker for that language
proof_policy: seam-test-first
contract_revision: 2
scope:
- Cargo.toml
- Cargo.lock
- docs/adr/README.md
- docs/adr/0013-data-driven-framework-manifests.md
- docs/adr/0017-serde-framework-manifest-formats.md
- src/db/writer.rs
- src/engine/languages/manifests.rs
- src/engine/linker/
- src/engine/languages/manifests/framework.rs
- src/engine/languages/mod.rs
- tests/html_profile.rs
- tests/python_semantics.rs
status: completed
subtasks:
- subtask_ref: required-manifest-tables
  title: Reject a manifest that omits receivers, builtins, filters, or routes; a complete fixture manifest loads
  status: completed
  evidence: cargo test --test html_profile framework_manifest -- --nocapture (exit 0; 2 passed, 0 failed). Four missing-table cases fail closed without publication; the complete fixture loads typed inline, scalar-array, and table-array values through DependencyRegistry, and is absent from the linker accessor when Django is undeclared. Blackboard measured_delta id 17.
- subtask_ref: declared-django-render
  title: 'render(request, ''templates/page.html'', {''title'': ''Welcome''}) resolves template.variable.title only when pyproject declares django; an undeclared project stays unresolved'
  status: completed
  evidence: cargo test --test html_profile rendered_template_context_keys_follow_exact_targets_and_includes -- --exact (1 passed, 0 failed). Public writer/SQLite assertions prove the declared Django render context resolves its exact template and included partial, while an undeclared project's same variable remains unresolved.
- subtask_ref: invalid-user-manifest
  title: Valid .toml/.yaml/.yml manifests load all four required arrays; missing or non-array sections, malformed YAML, and TOML dotted key/table collisions in either order fail before index publication
  status: completed
  evidence: 'Serde typed loader complete: valid .toml/.yaml/.yml manifests reach DependencyRegistry and its existing LanguageLinker accessor; required arrays and malformed inputs fail closed. Writer-seam contract cases prove missing/non-array arrays, malformed YAML/YML, duplicate TOML definitions, and both source orders of the nested [unused.receivers] / [unused] receivers collision fail without SQLite index publication. Verification: cargo check; cargo test --test html_profile framework_manifest (2 passed, 0 failed); cargo clippy --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; git diff --check. Blackboard measured_delta id 35.'
receipt:
  commit: faa332d7adcfd5f5b600f671995e1342f9f88f2b
  contract_revision: 2
  passed_at: 2026-10-07T17:11:01.027275912+00:00
  evidence:
    test_proof:
      command: cargo check; cargo test --test html_profile framework_manifest; cargo clippy --all-targets --all-features -- -D warnings
      exit_code: 0
      tests_passed: 2
      tests_failed: 0
      log: 'cargo check passed. Focused writer/SQLite seam: 2 passed, 0 failed across the complete-manifest and invalid-manifest contract tests. This covers successful TOML/YAML/YML loading; missing/non-array required sections; malformed YAML/YML; duplicate TOML definitions; both nested dotted-key/table collision source orders; and no index publication on invalid input. Strict all-target/all-feature clippy passed. cargo fmt --all -- --check and git diff --check passed.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Reviewed only the frozen snapshot `git show faa332d`. Its changed paths are within contract revision 2: Cargo.toml/Cargo.lock, the admitted ADR files, writer, framework loader and language/linker modules, and existing tests. The milestone contract is task authority outside this snapshot.'
        claims:
          applicable: true
          evidence: 'The typed manifest derives serde traits, requires all four Vec sections, denies unknown top-level fields, and dispatches TOML/YAML/YML to toml::from_str / serde_yaml::from_str (framework.rs:9-64). Loader filters those extensions and uses file_stem as the identifier (manifests.rs:455-490). The writer propagates try_collect errors before atomic publication: populate uses `?` at writer.rs:224-230, atomic_build only renames after successful populate at :477-492 and removes the staging directory on error at :501-503. Existing LanguageLinker.framework_manifest delegates to the declaration- and workspace-gated DependencyRegistry accessor (traits.rs:102-111; manifests.rs:278-291). Writer seam tests cover complete formats, missing/non-array sections, malformed YAML/YML, TOML collisions in both orders, and no output index on invalid manifests (html_profile.rs:1441-1716).'
        concurrency:
          applicable: true
          evidence: 'Current milestone task authority serializes shared-file work: framework-rule-migration depends on framework-manifest-schema-and-loader at docs/milestones/041-framework-manifests-and-parser-modularity.md:78, and manifest-equivalence depends on framework-rule-migration at :105.'
        project_isolation:
          applicable: true
          evidence: The accessor first checks declares_for_path, then indexes the manifest by workspace and framework (manifests.rs:278-291). The public integration fixture loads a declared Django manifest and verifies its file-stem name; an undeclared project returns no manifest (html_profile.rs:1460-1486 and :1622-1630).
        administration:
          applicable: true
          evidence: Reviewer worker codex-041-framework-manifest-schema-loader-review-20261007 differs from build worker codex-041-framework-manifest-schema-loader-build-20261007. Build receipt claim revision 12 records cargo check, `cargo test --test html_profile framework_manifest` (2 passed), strict clippy, formatting, and diff-check all passing for faa332d. Contract red proof is at d8036bc under revision 2. I did not run commands or modify files during this read-only review.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-LANGUAGE-LINKER: Framework manifests feed existing LanguageLinker implementations. They do not add a second linker.'
    - 'INV-GRAMMAR-BOUNDARY: Universal grammar, lexical scope, and imports stay in the language profile. Vue remains a language profile.'
    - 'INV-DECLARED-ACTIVATION: A framework manifest applies only when DependencyRegistry reports that the project declares the framework.'
    - 'INV-EQUIVALENCE: A migration preserves resolution-status multisets and edge triples (src, dst, kind). commitment_integrity stays deterministic. Evidence wording may change.'
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

### task: framework-rule-migration

```yaml
task_ref: framework-rule-migration
target: Move framework rules that are not universal grammar into manifests consumed by the current language linkers
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/engine/languages/html.rs
- src/engine/languages/manifests.rs
- src/engine/languages/manifests/
- src/engine/languages/python/
- src/engine/languages/typescript.rs
- src/engine/languages/html/
- src/engine/languages/typescript/
- src/engine/languages/vue.rs
- src/engine/linker.rs
- src/engine/linker/html_templates.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker/traits.rs
- tests/html_profile.rs
- tests/typescript_semantics.rs
depends_on:
- framework-manifest-schema-and-loader
subtasks:
- subtask_ref: django-single-and-shared-templates
  title: 'One Django view calling render(request, ''templates/page.html'', {''title'': ''Welcome''}) keeps template.variable.title resolved; two views targeting that template keep it ambiguous with zero context_provider edges'
  status: completed
  evidence: 'cargo test --test html_profile: exit 0, 45 passed. Production writer seam verifies one Django render context resolves template.variable.title and two renderers leave it ambiguous with zero context_provider edges (rendered_template_with_multiple_views_stays_ambiguous; rendered_template_context_keys_follow_exact_targets_and_includes). Django receiver recognition now comes from the declared manifest receiver mapping.'
- subtask_ref: alpine-magics-when-declared
  title: HTML script-island references this.$refs and this.$dispatch resolve as external Alpine builtins only when package.json declares alpinejs; otherwise both stay unresolved
  status: completed
  evidence: 'cargo test --test html_profile: exit 0, 45 passed. alpine_magic_members_require_declared_alpine_dependency observes this.$refs and this.$dispatch as external in the alpinejs package and unresolved in the neighboring project without alpinejs.'
- subtask_ref: nuxt-components-on-js-linker
  title: A TSX import { BaseWidgetCard } from '#components' resolves to components/base/WidgetCard.vue only when package.json declares nuxt; a Vue SFC tag without Nuxt stays unresolved
  status: completed
  evidence: 'cargo test --all-features --test typescript_semantics nuxt_: exit 0, 2 passed. nuxt_components_import_links_tsx_to_vue_only_for_declared_nuxt resolves #components to nuxt/components/base/WidgetCard.vue only in the Nuxt package; a Vue SFC tag without Nuxt remains unresolved. Nuxt computed/ref auto-import and local-shadow test also passes with its required nuxt.config.ts.'
- subtask_ref: jinja-include-and-filter
  title: With a declared Jinja2 FileSystemLoader, {% include 'partials/card.html' %} keeps its exact includes edge and rows|selectattr('active')|list keeps both filter references external after the rules move into the HTML linker manifest
  status: completed
  evidence: 'cargo test --test html_profile: exit 0, 45 passed. jinja_builtins_follow_imported_filesystem_loader_roots preserves the exact partial include edge and external selectattr/list filter references under the declared FileSystemLoader across cold/delta checks.'
- subtask_ref: shared-html-rules-preserve-equivalence
  title: In a project with no Django/Jinja declaration, an HTML template using `{% if active %}{% for row in rows %}{{ row|safe|urlencode }}{% endfor %}{% else %}{% endif %}` keeps the seven shared `template.tag.*` / `template.filter.*` references external through writer::build; the slice has 7 external and 0 unresolved rows, while manifest-only tags remain declaration-gated.
  status: completed
  evidence: 'cargo test --test html_profile shared_html_template_rules_stay_external_without_framework_declarations -- --nocapture (1 passed); full cargo test --test html_profile (46 passed). The writer::build seam asserts seven shared tag/filter references are external and Django-only csrf_token is unresolved without declaration. On pinned Commerce corpus ee93f8c0b4834406478dc7a1a9fb36e9cfd614db (same 8593fb5f…15b8 adapter, 3901 files, 65551 nodes, 302112 edges), baseline and candidate status counts match exactly (58 ambiguous, 130005 external, 121122 resolved, 43275 unresolved); sorted (src,dst,kind) triple digest matches: e57ab85b484da576bd37f9f50076d90f18551b506f9d1e374919a9d80b6dddbb. Two fresh candidate builds produced identical root 0a77e79023f930b90d08a71083f295be0b71fe5f5192eba9e427fccb1a65fedf.'
status: completed
receipt:
  commit: 7e77e0544085d6f15778b9aa14dd9efba540669f
  contract_revision: 2
  passed_at: 2026-10-07T18:50:21.957620766+00:00
  evidence:
    test_proof:
      command: cargo test --test html_profile && cargo test --all-features --test typescript_semantics nuxt_ && cargo test --test commitment_integrity
      exit_code: 0
      tests_passed: 62
      tests_failed: 0
      log: 46 html_profile tests, 2 Nuxt tests, and 14 commitment_integrity tests passed. The newly admitted seam test confirms shared HTML tags/filters remain external without Django/Jinja declarations while csrf_token stays unresolved. Strict cargo clippy --all-targets --all-features -- -D warnings, cargo fmt --all -- --check, and git diff --check passed. On pinned Commerce corpus, baseline/candidate resolution status counts match exactly (58 ambiguous, 130005 external, 121122 resolved, 43275 unresolved); sorted edge-triple digest matches (e57ab85b484da576bd37f9f50076d90f18551b506f9d1e374919a9d80b6dddbb). Two fresh candidate builds produced identical Merkle root 0a77e79023f930b90d08a71083f295be0b71fe5f5192eba9e427fccb1a65fedf.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent read-only reviewer audited frozen candidate snapshot 7e77e05 and confirmed all expanded changed paths stay inside contract revision 2's allowed paths. The reopened correction changes only src/engine/languages/html.rs and tests/html_profile.rs.
        claims:
          applicable: true
          evidence: All five subtasks are completed with accepted evidence. The new Html::builtin fallback restores shared HTML template tags/filters; the writer::build test proves seven shared references external without declarations and Django-only csrf_token unresolved. Pinned corpus status counts and sorted edge triples match baseline.
        concurrency:
          applicable: true
          evidence: The migration depends on completed framework-manifest-schema-and-loader. manifest-equivalence remains sequenced after this migration; its current contract claim does not run implementation code against the shared changed files.
        project_isolation:
          applicable: true
          evidence: DependencyRegistry declaration checks remain in place. The new test checks an undeclared project; prior Alpine, Nuxt, Django, and Jinja tests continue to cover declared activation and isolation.
        administration:
          applicable: true
          evidence: 'Focused verification passed: html_profile 46, Nuxt 2, commitment_integrity 14, strict Clippy, fmt check, diff check. Pinned corpus status counts are 58/130005/121122/43275; edge-triple SHA-256 matches baseline; two fresh candidate Merkle roots match. The full all-targets run still has one pre-existing failure in tests/mcp_context/tasks.rs:331 (fixture writes docs/milestones/010-tasks.md but reads root 010-tasks.md); that file is outside this task''s write scope and was not changed.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-LANGUAGE-LINKER: Framework manifests feed existing LanguageLinker implementations. They do not add a second linker.'
    - 'INV-GRAMMAR-BOUNDARY: Universal grammar, lexical scope, and imports stay in the language profile. Vue remains a language profile.'
    - 'INV-DECLARED-ACTIVATION: A framework manifest applies only when DependencyRegistry reports that the project declares the framework.'
    - 'INV-EQUIVALENCE: A migration preserves resolution-status multisets and edge triples (src, dst, kind). commitment_integrity stays deterministic. Evidence wording may change.'
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

### task: manifest-equivalence

```yaml
task_ref: manifest-equivalence
target: Prove status multisets and edge triples match the pre-migration corpus, and commitment_integrity stays green
proof_policy: seam-test-first
scope:
- src/engine/linker/
- tests/commitment_integrity.rs
- tests/html_profile.rs
depends_on:
- framework-rule-migration
subtasks:
- subtask_ref: status-and-endpoint-equivalence
  title: On /home/oleksii/ContextUnity/worktrees/commerce-release-update at corpus commit ee93f8c0b4834406478dc7a1a9fb36e9cfd614db, status counts and edge triples (src, dst, kind) match the pre-migration index from Forge commit 2bba3240933650c7dac8dead0e4f0d75f46147fb; evidence text may differ
  status: completed
  evidence: 'Pinned Commerce CLI build at ee93f8c0b4834406478dc7a1a9fb36e9cfd614db with adapter SHA-256 8593fb5f4f86a97e8f9f6e259e79f70657fc03b776890be817943f1bd01715b8 and baseline Forge commit 2bba3240933650c7dac8dead0e4f0d75f46147fb produced 3901 files, 65551 nodes, and 302112 edges. Baseline/candidate status multisets match exactly: 58 ambiguous, 130005 external, 121122 resolved, 43275 unresolved. Sorted (src,dst,kind) edge digest matches exactly: e57ab85b484da576bd37f9f50076d90f18551b506f9d1e374919a9d80b6dddbb. Both sides used the same recorded linked workspace heads; those dirty linked checkouts were not modified.'
- subtask_ref: commitment-determinism
  title: Two fresh builds of the same tree produce the same Merkle root after the manifest move
  status: completed
  evidence: 'Two fresh candidate builds of the pinned tree produced identical Merkle roots: 0a77e79023f930b90d08a71083f295be0b71fe5f5192eba9e427fccb1a65fedf both times. `cargo test --test commitment_integrity` passed all 14 tests. The per-build corpus evidence is recorded in the task''s measured_delta blackboard message.'
- subtask_ref: lifecycle-fixture-path-roundtrip
  title: After `task_submit` delivers the fixture, `tests/mcp_context/tasks.rs::task_stdio_lifecycle_submits_inline_evidence_in_independent_worktrees` reads `docs/milestones/010-tasks.md` (the same path written and synced), parses the milestone, and preserves `receipt.commit == Some(commit)` plus `DEFECT-E2E-001`; the focused real-stdio lifecycle test passes.
  status: completed
  evidence: 'cargo test --test mcp_context tasks::task_stdio_lifecycle_submits_inline_evidence_in_independent_worktrees -- --exact: pre-fix exit 101 at the stale root path; post-fix 1 passed, 0 failed using docs/milestones/010-tasks.md. The real MCP stdio lifecycle and receipt/deferred_defects assertions pass. Blackboard measured_delta id 58.'
status: completed
receipt:
  commit: 5da0502a376f9336de98d45b071f88fa8e029262
  contract_revision: 1
  passed_at: 2026-10-07T19:19:45.672832839+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets
      exit_code: 0
      tests_passed: 629
      tests_failed: 0
      log: 'Final post-fix verification: cargo test --all-targets passed 629 tests across 44 suites; 3 ignored, 0 failed. cargo clippy --all-targets --all-features -- -D warnings passed with no warnings. cargo test --test commitment_integrity passed 14/14. cargo fmt --all -- --check and git diff --check passed. The focused repaired MCP stdio lifecycle test also passed 1/1.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Independent review of snapshot 5da0502 (diff 83d2f5c..5da0502): only new lifecycle change is tests/mcp_context/tasks.rs, explicitly included by task scope extension. All other changed files remain within the preexisting 041 manifest/linker task scopes.'
        claims:
          applicable: true
          evidence: The final read at tests/mcp_context/tasks.rs:331 now matches fixture writes and sync under docs/milestones/010-tasks.md. The test still parses delivered state and asserts receipt commit, typed deferred defect, and completed status. Recorded production-seam test passes 1/1; final suite 629 passed/0 failed, commitment_integrity 14/14.
        concurrency:
          applicable: true
          evidence: framework-rule-migration dependency is completed. Lifecycle test uses independent temporary builder and reviewer workspaces; no conflicting active writer was found.
        project_isolation:
          applicable: true
          evidence: The test launches MCP against its temporary builder workspace and cleans up through its test harness; inspected sync and delivery paths remain confined to that workspace.
        administration:
          applicable: true
          evidence: Independent reviewer claim revision 9 uses worker codex-041-lifecycle-fixture-review-20261007, distinct from build worker codex-041-lifecycle-fixture-build-20261007. The lifecycle-fixture-path-roundtrip subtask and measured_delta evidence are recorded; no unresolved review gap.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-LANGUAGE-LINKER: Framework manifests feed existing LanguageLinker implementations. They do not add a second linker.'
    - 'INV-GRAMMAR-BOUNDARY: Universal grammar, lexical scope, and imports stay in the language profile. Vue remains a language profile.'
    - 'INV-DECLARED-ACTIVATION: A framework manifest applies only when DependencyRegistry reports that the project declares the framework.'
    - 'INV-EQUIVALENCE: A migration preserves resolution-status multisets and edge triples (src, dst, kind). commitment_integrity stays deterministic. Evidence wording may change.'
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

### Equivalence baseline

Build and retain the pre-migration index with Forge commit `2bba3240933650c7dac8dead0e4f0d75f46147fb` against `/home/oleksii/ContextUnity/worktrees/commerce-release-update` at corpus commit `ee93f8c0b4834406478dc7a1a9fb36e9cfd614db`, using `benchmarks/profiles/commerce-release-update.json`. The corpus worktree currently has a modified `forge-mcp.yaml` with SHA-256 `8593fb5f4f86a97e8f9f6e259e79f70657fc03b776890be817943f1bd01715b8`; use this same adapter configuration for the baseline and candidate indexes. Compare resolution-status counts and `(src, dst, kind)` edge triples; evidence text may differ.
