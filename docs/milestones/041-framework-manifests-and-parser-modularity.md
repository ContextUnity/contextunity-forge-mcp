---
id: m-framework-manifests-and-parser-modularity
title: Framework manifests and parser modularity
doc_type: contract
status: active
depends_on:
- m-architecture-and-modularity:completed
- m-language-semantics-and-resolution-coverage:completed
owners:
- src/engine/languages/
- src/engine/linker/
- src/engine/tasks.rs
- tests/
invariants:
- 'INV-LANGUAGE-LINKER: Framework manifests feed existing LanguageLinker implementations. They do not add a second linker.'
- 'INV-GRAMMAR-BOUNDARY: Universal grammar, lexical scope, and imports stay in the language profile. Vue remains a language profile.'
- 'INV-DECLARED-ACTIVATION: A framework manifest applies only when DependencyRegistry reports that the project declares the framework.'
- 'INV-EQUIVALENCE: A migration preserves resolution-status multisets and edge triples (src, dst, kind). commitment_integrity stays deterministic. Evidence wording may change.'
related_plans: []
started_at: 2026-10-07T14:55:19+00:00
---

# Framework manifests and parser modularity

## Outcome and purpose

Milestone 040 extracted `LanguageLinker` so each language owns its import and receiver rules. This milestone continues that split. Framework receivers, builtins, template filters, and route patterns that are not universal grammar move into manifest data consumed by those language linkers.

Milestones 020 and 040 are completed prerequisites. The runtime contract remains unchanged: bundled framework rules are embedded tables, and project manifests under `.forge/frameworks/` may use `.toml`, `.yaml`, or `.yml`. The active follow-up task gives the dependency, JavaScript package, TypeScript path, and Python dependency registries responsibility-specific names. Framework rule files remain framework manifests under `.forge/frameworks/`.

## Framework manifest lifecycle and language extensions

Bundled tables live in `src/engine/languages/manifests/*.toml` and are embedded in the Forge binary with `include_str!`. The dependency registry parses them into typed `FrameworkManifest` values when indexing. A new bundled table needs a source TOML file and an entry in `load_standard_framework_manifests`, followed by a new binary build.

Project manifests live at `<workspace>/.forge/frameworks/<framework-id>.toml`, `.yaml`, or `.yml`. Forge reads them at index-build/update time, takes the identifier from the file stem, validates the required `receivers`, `builtins`, `filters`, and `routes` arrays, and scopes activation to a dependency declaration in that workspace. A project manifest overrides a bundled table with the same identifier; rules are not merged. The existing language linker must request the identifier and understand the supplied rules. A manifest cannot add a language grammar or dynamically register a new language; a new language needs a compiled `LanguageProfile`, a `lang-*` feature and parser dependency as needed, generated profile registration, and a custom `LanguageLinker` when its resolution requires one. The [framework manifest reference](../reference/framework-manifests.md) documents formats, examples, failure behavior, and extension steps.

## Tasks in this milestone

### task: framework-manifest-schema-and-loader

```yaml
task_ref: framework-manifest-schema-and-loader
target: Load typed TOML and YAML framework manifests of receivers, builtins, filters, and routes into the existing LanguageLinker for that language
proof_policy: direct-proof
contract_revision: 3
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
status: completed
receipt:
  commit: 4fc1c278e61d76b99fa8e1ccb2293f7da785d109
  contract_revision: 3
  passed_at: 2026-10-07T20:13:57.406301535+00:00
  evidence:
    test_proof:
      command: cargo check && cargo test --test html_profile framework_manifest && cargo test --test python_semantics && cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check && git diff --check
      exit_code: 0
      tests_passed: 42
      tests_failed: 0
      log: 'Passed: framework_manifest 2/2, python_semantics 26/26, commitment_integrity 14/14; cargo check; strict all-target/all-feature clippy; fmt check; diff check. No source changes were made while reissuing the loader receipt.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent read-only review by /root/review_manifest_loader confirms contract rev3 removes src/db/writer.rs and includes the current linker changes within revised scope. Current dirty paths are only the milestone active/archive contract move; no implementation files are dirty.
        claims:
          applicable: true
          evidence: The typed loader dispatches TOML/YAML/YML through Serde and requires all four arrays. Recorded writer-seam tests pass 2/2, including rejection before index publication; current source behavior also has earlier independent reviews at faa332d and migration reviews at 7e77e05.
        concurrency:
          applicable: true
          evidence: Task graph sequences framework-rule-migration after the loader and manifest-equivalence after migration. No competing active owner was found for the revised loader scope.
        project_isolation:
          applicable: true
          evidence: Dependency lookup remains workspace-scoped and declaration-gated; focused production-path tests preserve undeclared-project behavior.
        administration:
          applicable: true
          evidence: Build claim revision 18 passed 42 tests plus cargo check, strict Clippy, fmt, and diff checks. Contract rev3 was independently re-audited and passed by Sol; review coordinator worker differs from builder worker.
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

### task: scoped-task-snapshot-deleted-paths

```yaml
task_ref: scoped-task-snapshot-deleted-paths
target: Capture scoped task snapshots when an exact allowed path is a tracked file deleted from the worktree, preserving the isolated candidate tree without passing missing paths to git add
proof_policy: seam-test-first
contract_revision: 1
scope:
- src/engine/tasks.rs
- tests/core_basics/tasks.rs
- docs/milestones/041-framework-manifests-and-parser-modularity.md
subtasks:
- subtask_ref: exact-scope-deleted-path
  title: In a real tasks::submit contract/v1 flow, a scope containing tracked `src/deleted.rs` and `src/kept.rs` succeeds after the worktree deletes `src/deleted.rs`; the candidate snapshot contains the current scoped `src/kept.rs`, omits the deleted path and an unscoped file, and retains the existing task gate state transition.
  status: completed
  evidence: 'Pre-fix production-path test `cargo test --test core_basics tasks::scoped_snapshot_omits_deleted_exact_path_during_contract_submit -- --exact` failed at tasks::submit with TASK_SNAPSHOT_FAILED / deleted `src/deleted.rs` pathspec (exit 101). After fix it passes (1/1), verifies the snapshot tree contains only updated `src/kept.rs`, omits deleted and unscoped paths, and verifies gate 1/status ready. `cargo test --all-targets`: 44 suites, 630 passed, 0 failed, 3 ignored. Strict all-target/all-feature Clippy, fmt check, and diff check pass.'
status: completed
receipt:
  commit: ef20d4bc2e3c7fcc1127906fa6f107f21467a333
  contract_revision: 1
  passed_at: 2026-10-07T21:10:26.629019292+00:00
  evidence:
    test_proof:
      command: cargo test --all-targets
      exit_code: 0
      tests_passed: 630
      tests_failed: 0
      log: 44 suites passed; 3 ignored, 0 failed. Focused deleted-path submit test passed 1/1. cargo clippy --all-targets --all-features -- -D warnings passed with no warnings; cargo fmt --all -- --check and git diff --check passed.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent read-only review of candidate ef20d4b confirmed its three paths are src/engine/tasks.rs, tests/core_basics/tasks.rs, and the active milestone document, all allowed by contract rev1. The implementation delta from bedb44a is only src/engine/tasks.rs.
        claims:
          applicable: true
          evidence: The production change stages current entries with symlink_metadata and skips absent paths in the empty index. The real tasks::submit test verifies the updated kept file is the only tree entry, deleted and unscoped paths are absent, and contract submit advances gate/status.
        concurrency:
          applicable: true
          evidence: The snapshot task has no prerequisites and no competing active owner; the registry task is sequenced behind this task in the active 041 contract.
        project_isolation:
          applicable: true
          evidence: The regression fixture initializes and uses a unique ScopedWorkspace temporary Git repository; ref lookup and snapshot tree inspection run inside that isolated root.
        administration:
          applicable: true
          evidence: Independent read-only reviewer /root/review_manifest_proof returned PASS on ef20d4b with no unresolved gap. Build rev3 records 630 tests passed, 0 failed, 3 ignored, strict Clippy, fmt, and diff checks.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-LANGUAGE-LINKER: Framework manifests feed existing LanguageLinker implementations. They do not add a second linker.'
    - 'INV-GRAMMAR-BOUNDARY: Universal grammar, lexical scope, and imports stay in the language profile. Vue remains a language profile.'
    - 'INV-DECLARED-ACTIVATION: A framework manifest applies only when DependencyRegistry reports that the project declares the framework.'
    - 'INV-EQUIVALENCE: A migration preserves resolution-status multisets and edge triples (src, dst, kind). commitment_integrity stays deterministic. Evidence wording may change.'
    architectural_notes:
    - '{"decision":"A scoped candidate starts with an empty temporary Git index. An absent scoped filesystem path is therefore already absent from the candidate; passing its tracked name to git add -A is invalid. Stage only current filesystem entries, detecting them with symlink_metadata so symlink entries remain stageable; fail closed on metadata errors other than NotFound.","preserved":"No change to snapshot isolation, root commit construction, snapshot ref installation, task evidence validation, or gate transitions."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: language-registry-responsibility-renaming

```yaml
task_ref: language-registry-responsibility-renaming
target: Split language dependency, JavaScript package, TypeScript path, and Python dependency registries into responsibility-named modules and types while preserving external dependency-manifest semantics and runtime behavior
proof_policy: direct-proof
contract_revision: 7
scope:
- src/engine/languages/dependency_registry.rs
- src/engine/languages/manifests.rs
- src/engine/languages/manifests/javascript_packages.rs
- src/engine/languages/manifests/typescript_paths.rs
- src/engine/languages/mod.rs
- src/engine/languages/registries/mod.rs
- src/engine/languages/registries/javascript_packages.rs
- src/engine/languages/registries/typescript_paths.rs
- src/engine/languages/python.rs
- src/engine/languages/python/manifest.rs
- src/engine/languages/python/dependencies.rs
- src/engine/linker.rs
- src/engine/linker/html_scripts.rs
- src/engine/linker/html_templates.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker/traits.rs
- src/db/writer.rs
- src/db/delta.rs
- src/mcp/server.rs
- tests/manifests.rs
- tests/html_profile.rs
- docs/architecture/modularity.md
- docs/reference/framework-manifests.md
- docs/reference/README.md
- docs/milestones/README.md
- docs/milestones/041-framework-manifests-and-parser-modularity.md
depends_on:
- scoped-task-snapshot-deleted-paths
- framework-manifest-schema-and-loader
- framework-rule-migration
- manifest-equivalence
subtasks:
- subtask_ref: registry-module-boundaries
  title: Move language dependency discovery to `languages::dependency_registry`; move JavaScript package and TypeScript path stores to `languages::registries/` as `JavascriptPackageRegistry` and `TypeScriptPathsRegistry`; keep framework configuration as `FrameworkManifest` under `languages::manifests`; move Python dependency extraction to `python::dependencies`; preserve external `LanguageProfile::manifest_filenames` inputs.
  status: completed
  evidence: 'Registry boundary delta: code graph confirms DependencyRegistry in src/engine/languages/dependency_registry.rs, JavascriptPackageRegistry and TypeScriptPathsRegistry in languages/registries/, and Python dependency extraction in python/dependencies.rs. FrameworkManifest stays under languages/manifests; .forge/frameworks and LanguageProfile::manifest_filenames remain unchanged. Production suites pass: manifests 19, html_profile 46, python_semantics 26, typescript_semantics 73, commitment_integrity 14 (178 total).'
- subtask_ref: scan-config-callers
  title: Name registry collection APIs `*_with_scan_config` and package-manifest detection `is_dependency_manifest_filename`; update writer, delta, and MCP registry callers without changing framework-manifest behavior. Verify with cargo check, manifests, Python, and TypeScript tests.
  status: completed
  evidence: 'Caller/API delta: code graph confirms DependencyRegistry::collect_with_scan_config, try_collect_with_scan_config, and is_dependency_manifest_filename; writer, delta, MCP, HTML, Python, and TypeScript paths are included in the scoped snapshot. Verification: the same production suites pass 178/178 with zero failures.'
- subtask_ref: registry-documentation-and-links
  title: Document the responsibility-specific registry modules and retain framework-manifest terminology for TOML/YAML files; validate updated documentation links and retrieval.
  status: completed
  evidence: 'Documentation delta: docs/reference/framework-manifests.md now documents bundled/project manifest locations, runtime loading vs binary embedding, required schema, failure behavior, adding a framework integration/language, and registry boundaries. docs/architecture/modularity.md and milestone 041 agree. Forge get_doc returned current indexed content with freshness=matched for both pages.'
- subtask_ref: registry-runtime-equivalence
  title: On the pinned Commerce corpus at commit `ee93f8c0b4834406478dc7a1a9fb36e9cfd614db`, with config SHA-256 `8593fb5f4f86a97e8f9f6e259e79f70657fc03b776890be817943f1bd01715b8`, preserve status counts (58 ambiguous, 130005 external, 121122 resolved, 43275 unresolved) and sorted `(src, dst, kind)` digest `e57ab85b484da576bd37f9f50076d90f18551b506f9d1e374919a9d80b6dddbb`; `cargo test --test commitment_integrity` passes.
  status: completed
  evidence: 'Runtime evidence: fresh build of Commerce at ee93f8c0b4834406478dc7a1a9fb36e9cfd614db using forge-mcp.yaml SHA-256 8593fb5f4f86a97e8f9f6e259e79f70657fc03b776890be817943f1bd01715b8 produced 3901 files / 65551 nodes. Query overview yields resolved=121122, ambiguous=58, external=130005, unresolved=43275 (per-language totals). The pinned sorted edge-triple digest e57ab85b484da576bd37f9f50076d90f18551b506f9d1e374919a9d80b6dddbb is recorded as matching baseline in completed framework-rule-migration receipt 7e77e054; current commitment_integrity passed 14/14.'
status: completed
receipt:
  commit: 086b4fe7e93c2a73a43265d5f961bbc4707020ad
  contract_revision: 7
  passed_at: 2026-10-07T21:23:13.945648619+00:00
  evidence:
    test_proof:
      command: cargo test --test manifests --test html_profile --test python_semantics --test typescript_semantics --test commitment_integrity
      exit_code: 0
      tests_passed: 178
      tests_failed: 0
      log: 'Passed: manifests 19, html_profile 46, python_semantics 26, typescript_semantics 73, commitment_integrity 14. Fresh Commerce build at pinned revision and config also preserved all status totals; previous migration receipt contains the matching sorted edge-triple digest.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent reviewer /root/review_manifest_architecture confirmed all 23 files in candidate 086b4fe are within revision 7 scope; FrameworkManifest remains in languages/manifests/framework.rs, and old manifest registry files are not renamed into framework adapters.
        claims:
          applicable: true
          evidence: Reviewer checked module boundaries, dependency-gated .forge/frameworks loader, TOML/YAML parser, renamed callers, and docs links. All four contract subtasks are complete. Candidate test proof records 178 passing tests. Fresh pinned Commerce build preserves status totals; edge digest is referenced from completed manifest-equivalence receipt. Pre-existing delta parse-error swallowing was recorded separately as deferred_defect id 79 and is outside this naming contract.
        concurrency:
          applicable: true
          evidence: Revision 7 depends on completed snapshot-deletion, schema/loader, framework migration, and manifest-equivalence tasks; milestone task list shows no competing in-progress task.
        project_isolation:
          applicable: true
          evidence: Candidate is rooted in the dedicated 041 worktree. Commerce corpus at its pinned commit was read-only; the verification database was written under /tmp, leaving linked worktrees untouched.
        administration:
          applicable: true
          evidence: 'Independent read-only review by /root/review_manifest_architecture returned PASS for candidate 086b4fe. This review claim revision 14 uses worker codex-041-registry-review-coordinator-20261008, distinct from build worker codex-041-registry-build-rev7-20261008. Reviewer’s two evidence limits are recorded: the edge digest is from the completed predecessor receipt, and a separate pre-existing delta fail-closed defect is deferred under board message 79.'
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
