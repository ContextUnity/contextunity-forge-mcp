---
id: m-framework-manifests-and-parser-modularity
title: Data-driven framework manifests on the LanguageLinker seam
doc_type: contract
status: active
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
---

# Data-driven framework manifests on the LanguageLinker seam

## Outcome and purpose

Milestone 040 extracted `LanguageLinker` so each language owns its import and receiver rules. This milestone continues that split. Framework receivers, builtins, template filters, and route patterns that are not universal grammar move into manifests those language linkers already consume.

Milestones 020 and 040 are completed prerequisites. The first accepted task claim activates this planned milestone and records `started_at`. Standard manifests are static tables. User files at `.forge/frameworks/` may use `.toml`, `.yaml`, or `.yml`; serde-backed parsing validates them before publication. Proof is a public writer and SQLite seam, not `direct-proof`.

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
target: "Move framework rules that are not universal grammar into manifests consumed by the current language linkers"
proof_policy: seam-test-first
scope:
- src/engine/languages/python/
- src/engine/languages/html/
- src/engine/languages/typescript/
- src/engine/languages/vue.rs
- tests/html_profile.rs
- tests/typescript_semantics.rs
depends_on: [framework-manifest-schema-and-loader]
status: blocked
subtasks:
- subtask_ref: django-single-and-shared-templates
  title: "One Django view rendering templates/page.html with key title stays resolved; two views of that template stay ambiguous with zero context_provider edges"
  status: pending
- subtask_ref: alpine-magics-when-declared
  title: "HTML island expressions $refs and $dispatch resolve as external Alpine builtins only when package.json declares alpinejs; otherwise they stay unresolved"
  status: pending
- subtask_ref: nuxt-components-on-js-linker
  title: "A Nuxt components/ directory resolves a component import only when package.json declares nuxt, through the JavaScript or TypeScript linker; a Vue SFC without nuxt does not use that directory rule"
  status: pending
- subtask_ref: jinja-include-and-filter
  title: "Jinja include and a registered filter keep their current resolved status after the rule moves into the HTML language linker's manifest"
  status: pending
```

### task: manifest-equivalence

```yaml
task_ref: manifest-equivalence
target: "Prove status multisets and edge triples match the pre-migration corpus, and commitment_integrity stays green"
proof_policy: seam-test-first
scope:
- src/engine/linker/
- tests/commitment_integrity.rs
- tests/html_profile.rs
depends_on: [framework-rule-migration]
status: blocked
subtasks:
- subtask_ref: status-and-endpoint-equivalence
  title: "On /home/oleksii/ContextUnity/worktrees/commerce-release-update at corpus commit ee93f8c0b4834406478dc7a1a9fb36e9cfd614db, status counts and edge triples (src, dst, kind) match the pre-migration index from Forge commit 2bba3240933650c7dac8dead0e4f0d75f46147fb; evidence text may differ"
  status: pending
- subtask_ref: commitment-determinism
  title: "Two fresh builds of the same tree produce the same Merkle root after the manifest move"
  status: pending
```

### Equivalence baseline

Build and retain the pre-migration index with Forge commit `2bba3240933650c7dac8dead0e4f0d75f46147fb` against `/home/oleksii/ContextUnity/worktrees/commerce-release-update` at corpus commit `ee93f8c0b4834406478dc7a1a9fb36e9cfd614db`, using `benchmarks/profiles/commerce-release-update.json`. The corpus worktree currently has a modified `forge-mcp.yaml` with SHA-256 `8593fb5f4f86a97e8f9f6e259e79f70657fc03b776890be817943f1bd01715b8`; use this same adapter configuration for the baseline and candidate indexes. Compare resolution-status counts and `(src, dst, kind)` edge triples; evidence text may differ.
