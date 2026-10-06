---
id: m-framework-manifests-and-parser-modularity
title: Data-driven framework manifests on the LanguageLinker seam
doc_type: contract
status: planned
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
---

# Data-driven framework manifests on the LanguageLinker seam

## Outcome and purpose

Milestone 040 extracted `LanguageLinker` so each language owns its import and receiver rules. This milestone continues that split. Framework receivers, builtins, template filters, and route patterns that are not universal grammar move into manifests those language linkers already consume.

Work starts after milestone 020 is completed. Every task below is `blocked` until then. Standard manifests are static tables. A user file at `.forge/frameworks/*.toml` is an explicit optional read with fail-closed validation. Proof is a public writer and SQLite seam, not `direct-proof`.

## Tasks in this milestone

### task: framework-manifest-schema-and-loader

```yaml
task_ref: framework-manifest-schema-and-loader
target: "Load a typed manifest of receivers, builtins, filters, and routes into the existing LanguageLinker for that language"
proof_policy: seam-test-first
scope:
- src/engine/languages/manifests.rs
- src/engine/linker/
- tests/html_profile.rs
- tests/python_semantics.rs
status: blocked
subtasks:
- subtask_ref: required-manifest-tables
  title: "Reject a manifest that omits receivers, builtins, filters, or routes; a complete fixture manifest loads"
  status: blocked
- subtask_ref: declared-django-render
  title: "render(request, 'templates/page.html', {'title': 'Welcome'}) resolves template.variable.title only when pyproject declares django; an undeclared project stays unresolved"
  status: blocked
- subtask_ref: invalid-user-manifest
  title: "An invalid .forge/frameworks/*.toml fails the build closed and writes no framework edges"
  status: blocked
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
status: blocked
subtasks:
- subtask_ref: django-single-and-shared-templates
  title: "One Django view rendering templates/page.html with key title stays resolved; two views of that template stay ambiguous with zero context_provider edges"
  status: blocked
- subtask_ref: alpine-magics-when-declared
  title: "HTML island expressions $refs and $dispatch resolve as external Alpine builtins only when package.json declares alpinejs; otherwise they stay unresolved"
  status: blocked
- subtask_ref: nuxt-components-on-js-linker
  title: "A Nuxt components/ directory resolves a component import only when package.json declares nuxt, through the JavaScript or TypeScript linker; a Vue SFC without nuxt does not use that directory rule"
  status: blocked
- subtask_ref: jinja-include-and-filter
  title: "Jinja include and a registered filter keep their current resolved status after the rule moves into the HTML language linker's manifest"
  status: blocked
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
status: blocked
subtasks:
- subtask_ref: status-and-endpoint-equivalence
  title: "On the reference corpus, resolution status counts and edge triples (src, dst, kind) match the pre-migration index; evidence text may differ"
  status: blocked
- subtask_ref: commitment-determinism
  title: "Two fresh builds of the same tree produce the same Merkle root after the manifest move"
  status: blocked
```
