---
id: m-language-parsers-and-declarative-framework-packs
title: Declarative framework packs, rule externalization, and 3-tier language parsers
doc_type: contract
status: planned
depends_on:
- m-declarative-framework-packs-and-cold-build-pipeline
owners:
- src/engine/languages/
- src/engine/ast/
- src/engine/linker/
- docs/reference/
- docs/architecture/
- skills/contextunity-forge/references/
invariants:
- 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository (commerce-release-update) must not regress across runtime parser and framework extraction changes.'
- 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions with explicit collision precedence (project rules precede bundled rules) and support replace = true total substitution.'
- 'INV-RULE-EXTERNALIZATION: Class hierarchy receivers, route decorators, and route call extraction rules execute from declarative manifest definitions without hardcoded Rust match branching.'
- 'INV-PARSER-UNIFICATION: New language profiles adopt SyntaxGrammarSpec declarative syntax kind tables (~80 LOC) with shared SinglePassExtractor traversal, replacing ad-hoc recursive AST walks.'
- 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic for unchanged semantics and pass cargo test --all-targets; newly admitted semantic facts deterministically increment edge counts without altering unrelated domain digests.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
- docs/plans/declarative-tree-sitter-queries-study.md
---

# Declarative framework packs, rule externalization, and 3-tier language parsers

## Outcome and purpose

Deliver comprehensive declarative language and framework extensibility for ContextUnity Forge MCP:
1. Externalize hardcoded framework rules from Rust code into TOML/YAML manifests: migrate base class receiver hierarchies (`django.db.models.Model`, `BaseCommand`, `HttpRequest`, `pydantic.BaseModel`, `GridViewSpec`) from `python/receivers.rs` into `[[class_hierarchy_receivers]]`, and route decorators/calls into `[[route_decorators]]` / `[[route_calls]]` covering Django (`path`), FastAPI (`@app.get`), and NestJS (`@Get`), wired into `InheritanceIndexes::build`.
2. Implement cross-layer framework relations: admit fact extractors in `src/engine/languages/python.rs`, `typescript/frameworks.rs`, and `html.rs` to wire Django `{% url 'name' %}` backend route resolution, CBV `template_name` (`renders_template`), and Alpine.js `Alpine.store('name', ...)` ↔ `$store.name.*` lookup.
3. Establish 3-tier language architecture: declarative `SyntaxGrammarSpec` (~80 LOC) for rapid new language onboarding, and shared `SinglePassExtractor` with enter/exit `ScopeFrame` stack in `src/engine/ast/mod.rs` to replace redundant function body traversals.
4. Co-evolve architecture documentation in `docs/architecture/indexing.md` and reference documentation in `docs/reference/framework-manifests.md`.

---

## Tasks in this milestone

### task: rust-to-toml-rule-externalization

```yaml
task_ref: rust-to-toml-rule-externalization
target: "Externalize hardcoded class receivers, route decorators, route calls, and dependency injection rules into declarative manifest sections and wire linker consumption"
proof_policy: seam-test-first
contract_revision: 3
scope:
  - src/engine/languages/manifests/framework.rs
  - src/engine/languages/manifests/django.toml
  - src/engine/languages/manifests/gridviewspec.toml
  - src/engine/languages/python/receivers.rs
  - src/engine/languages/context.rs
  - src/engine/languages/dependency_registry.rs
  - src/engine/languages/python.rs
  - src/engine/languages/typescript/mod.rs
  - src/engine/languages/html/htmx.rs
  - src/engine/linker/indexing.rs
  - src/engine/linker/mod.rs
  - src/engine/ast/routes.rs
  - docs/reference/framework-manifests.md
  - docs/architecture/indexing.md
  - skills/contextunity-forge/references/
  - tests/languages/manifests.rs
  - tests/languages/python/
status: ready
subtasks:
  - subtask_ref: externalize-class-hierarchy-receivers
    title: "Support [[class_hierarchy_receivers]] in FrameworkManifest and wire InheritanceIndexes::build in linker/indexing.rs for Django, TestCase hierarchies, and GridViewSpec"
    status: pending
  - subtask_ref: externalize-route-decorators-and-calls
    title: "Support [[route_decorators]], [[route_calls]], and [[di_calls]] in FrameworkManifest, wire context propagation in python.rs and typescript/mod.rs, and extract routes for FastAPI (Depends, APIRouter prefix composition), NestJS, Django URL namespaces/include, and GridViewSpec"
    status: pending
  - subtask_ref: co-evolve-skill-references
    title: "Update shared skills/contextunity-forge/references/ with verified examples for all new manifest sections"
    status: pending
```

#### Acceptance

1. `class_hierarchy_receivers` in manifest TOML dynamically register framework receiver methods (including GridViewSpec `GridView` and TestCase hierarchies) and are actively consumed by `InheritanceIndexes::build` without hardcoded Rust pattern matching.
2. `[[route_decorators]]`, `[[route_calls]]`, and `[[di_calls]]` in manifest TOML propagate from `DependencyRegistry` through `LanguageContext` to extract backend routes and DI parameters for Django (including `include("app.urls")` prefix concatenation), FastAPI (`Depends` and `APIRouter(prefix)`), NestJS, and custom frameworks.
3. `skills/contextunity-forge/references/` and `docs/reference/framework-manifests.md` provide matching, syntax-valid examples of all manifest sections.
4. `cargo test --test languages` passes with zero regression in route and receiver resolution.

---

### task: two-phase-semantic-fact-bus

```yaml
task_ref: two-phase-semantic-fact-bus
target: "Implement two-phase semantic fact bus for Django {% url %}, CBV template_name, Alpine.store, template_attributes, Pinia, and GridViewSpec with measurable coverage proof"
proof_policy: seam-test-first
contract_revision: 3
scope:
  - src/engine/languages/python.rs
  - src/engine/languages/typescript/frameworks.rs
  - src/engine/languages/html.rs
  - src/engine/languages/html/htmx.rs
  - src/engine/linker/framework_relations.rs
  - docs/architecture/indexing.md
  - tests/languages/html/
  - tests/languages/manifests.rs
status: ready
subtasks:
  - subtask_ref: alpine-store-fact-extraction-and-linking
    title: "Emit named Alpine.store and Pinia defineStore registration identities in typescript/frameworks.rs, link to template $store.<name>.<member>, and support code-marker activation for x-data and hx-get"
    status: pending
  - subtask_ref: django-template-and-route-linking
    title: "Extract CBV template_name and [[template_attributes]] (renders_template, uses_model, spec) in python.rs, and link {% url 'name' %} in html.rs and htmx.rs to backend routes"
    status: pending
  - subtask_ref: component-and-controller-injection-linking
    title: "Link Vue provide/inject, NestJS constructor-injected members to fields, and $dispatch events to @event and HX-Trigger listeners"
    status: pending
  - subtask_ref: qualitative-coverage-proof
    title: "Verify and measure qualitative graph coverage gain: edge count increases, zero unresolved references for registered templates/stores, and verified round-trip to GridViewSpec"
    status: pending
```

#### Acceptance

1. `Alpine.store('cart', ...)` and Pinia `defineStore('cart', ...)` produce named store facts that resolve template `$store.cart.open()` references; template code-markers (`x-data`, `hx-get`) activate relevant rule processing even without package manifest declarations.
2. Django CBV `template_name = "..."` and declarative `[[template_attributes]]` create typed `renders_template`, `uses_model`, and `spec` edges to HTML templates and model definitions.
3. Django `{% url 'name' %}` references in templates resolve to backend route definitions across nested `include()` paths.
4. Vue `provide`/`inject`, NestJS constructor-injected properties, and `$dispatch` ↔ `@event`/`HX-Trigger` relationships produce resolved semantic edges.
5. Qualitative coverage gain is explicitly proven: test suites demonstrate increased resolved edge counts with zero regressions in baseline relations.
6. Persisted producer-to-linker integration tests pass in `tests/languages/html/`.

---

### task: compiled-rule-ir-and-three-tier-profiles

```yaml
task_ref: compiled-rule-ir-and-three-tier-profiles
target: "Compile framework manifests into typed RuleIR indexed by Tree-sitter kind_id, add rule coverage verification, and establish Three-Tier language profiles"
proof_policy: seam-test-first
contract_revision: 3
scope:
  - src/engine/languages/mod.rs
  - src/engine/languages/manifests/mod.rs
  - src/engine/languages/manifests/framework.rs
  - src/engine/ast/mod.rs
  - src/engine/ast/visitor.rs
  - src/engine/languages/python.rs
  - src/engine/languages/rust.rs
  - src/engine/languages/typescript/mod.rs
  - src/engine/languages/vue.rs
  - src/engine/languages/vue/setup_facts.rs
  - src/engine/languages/vue/template.rs
  - docs/architecture/modularity.md
  - docs/architecture/indexing.md
  - tests/languages/
  - tests/core/
status: ready
subtasks:
  - subtask_ref: compile-framework-rules-to-kind-ir
    title: "Compile framework manifest rules into typed RuleIR dispatch tables indexed by Tree-sitter kind_id, re-compiling automatically on rule change"
    status: pending
  - subtask_ref: rule-coverage-verification-and-diagnostics
    title: "Implement rule coverage verification diagnostics warning or reporting if effective rule coverage degrades upon manifest substitution"
    status: pending
  - subtask_ref: syntax-grammar-spec-definition
    title: "Define SyntaxGrammarSpec struct for declaration, reference, scope, import, member-access, and comment kind mapping (~80 LOC)"
    status: pending
  - subtask_ref: single-pass-extractor-with-scope-frames
    title: "Implement shared SinglePassExtractor in ast/mod.rs and visitor.rs with enter/exit ScopeFrame stack, integrating Python, Rust, and TS binding and value-flow hooks"
    status: pending
```

#### Acceptance

1. Framework manifest rules compile into an in-memory `RuleIR` dispatch table keyed by Tree-sitter `kind_id`, avoiding nested TOML string matching on hot AST loops.
2. Changing a manifest triggers automatic re-compilation; rule verification diagnostics alert if symbol/route coverage drops on test suites.
3. `SyntaxGrammarSpec` enables concise language kind registration (~80 LOC) for declarations, references, scopes, imports, member access, and comments.
4. `SinglePassExtractor` traverses declaration bodies in a single DFS pass with `ScopeFrame` enter/exit lifecycle, integrating Python, Rust, and TypeScript binding and value-flow hooks without repeated subtree walks.
5. Existing language test suites pass with identical Merkle commitment digests for unmodified fixture sources.
6. `docs/architecture/modularity.md` is updated to formally document the Three-Tier language classification architecture (Tier 1: Declarative `SyntaxGrammarSpec` kind tables; Tier 2: Shared `SinglePassExtractor` AST traversal; Tier 3: Polyglot Semantic Linker). Declarative SCM queries remain an opt-in research direction governed by `docs/plans/declarative-tree-sitter-queries-study.md`.
