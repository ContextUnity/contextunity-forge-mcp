---
id: m-language-semantics-and-resolution-coverage
title: "Language semantics and resolution coverage"
doc_type: contract
status: active
depends_on: []
owners:
  - src/engine/
  - src/core/
  - tests/
invariants:
  - "INV-NO-NOISE: Non-symbol AST tokens (wildcards, lifetimes, numeric tuple indexes) must not generate reference records."
  - "INV-ROUTE-PRECISION: Only explicit web framework calls and Django patterns with valid route paths generate route nodes and handles edges."
  - "INV-LEXICAL-SCOPING: Python and standard language imports must maintain strict lexical and line-ordered scoping."
  - "INV-MERKLE-DETERMINISM: Merkle tree commitment integrity must pass at all times across cold and incremental builds."
related_plans:
  - docs/plans/tool-performance-and-db-optimization.md
  - docs/plans/architecture-and-modularity.md
---

# Language semantics and resolution coverage

## Outcome and purpose

Improve resolution coverage across supported languages (Rust, Python, TypeScript/JavaScript, Vue, HTML), eliminating spurious `unresolved` symbols and removing erroneous AST graph elements (e.g. false HTTP routes from dictionary lookups, non-symbol tokens, unlinked built-in APIs).

## Tasks in this milestone

### task: rust-semantics-and-noise-filtering

```yaml
task_ref: rust-semantics-and-noise-filtering
target: "Eliminate AST noise (_, lifetimes, tuple index) and expand standard library semantics and Rust constructors"
proof_policy: seam-test-first
scope:
  - src/engine/ast/relations.rs
  - src/engine/languages/rust.rs
  - src/engine/languages/rust/value_flow.rs
  - src/engine/linker/value_flow.rs
  - src/engine/linker.rs
  - tests/
status: completed
receipt:
  tested_commit: "7ca68c3"
  passed_at: "2026-10-01T16:38:40Z"
  evidence:
    - "cargo test --test commitment_integrity (12/12 passed)"
    - "cargo test --all-targets (100% passed)"
    - "cargo clippy --all-targets --all-features -- -D warnings (0 warnings)"
    - "unresolved count reduced from 18,493 to 14,596 (-21%)"
    - "resolved count increased from 7,737 to 9,771 (+26.3%)"
```

Eliminated AST noise from wildcard expressions (`_`), type lifetimes (`'a`), and numeric tuple member indexing (`self.0`). Added Rust standard library builtins and associated functions (`Box`, `Arc`, `drop`, `Vec::new`, `String::from`, `Path::new`, `PathBuf::from`). Implemented type-flow inference for constructor calls (`Workspace::new()`, `.open()`, `.create()`) in local bindings.

### task: ast-route-registration-precision

```yaml
task_ref: ast-route-registration-precision
target: "Eliminate spurious HTTP routes from dict.get() and Map.get() invocations in routes.rs"
proof_policy: seam-test-first
scope:
  - src/engine/ast/routes.rs
  - tests/ast_extractors.rs
status: completed
receipt:
  tested_commit: "7ca68c3"
  passed_at: "2026-10-01T16:38:40Z"
  evidence:
    - "cargo test --test ast_extractors (5/5 passed)"
    - "tests/ast_extractors.rs: python_routes_preserve_http_decorators_and_django_patterns_as_mapping_calls_remain_calls"
    - "tests/ast_extractors.rs: javascript_routes_preserve_middleware_and_framework_decorators_with_finite_receivers"
```

Restricted route registration in `routes.rs` so that only calls starting with `/` or `^` or Django `path`/`re_path` patterns on verified router/app receivers (`app`, `router`, `server`, `api`, `bp`, `blueprint`, `route`) produce route nodes. Disallowed literal handlers (`""`, `[]`, `{}`, primitives), eliminating thousands of false route nodes and unresolved literal handles.

### task: typescript-dom-and-testing-builtins

```yaml
task_ref: typescript-dom-and-testing-builtins
target: "Expand TypeScript/JavaScript profile with DOM, Fetch, Playwright web standards, and array methods"
proof_policy: seam-test-first
scope:
  - src/engine/languages/typescript.rs
  - tests/typescript_semantics.rs
status: completed
receipt:
  passed_at: "2026-10-01T18:24:00Z"
  evidence:
    - "cargo test --test typescript_semantics (22/22 passed)"
    - "added browser global types and Web API method symbols"
```

Provides browser globals (`document.querySelector`, `document.getElementById`, `HTMLElement`, `Element`, `fetch`, `window.addEventListener`) and collection methods (`push`, `map`, `filter`, `slice`) in TypeScript/JavaScript builtins so standard web APIs and Playwright tests resolve cleanly.

### task: vue-compiler-macros-and-script-setup

```yaml
task_ref: vue-compiler-macros-and-script-setup
target: "Support Vue 3 compiler macros (<script setup>, defineProps, defineEmits, ref, computed)"
proof_policy: seam-test-first
scope:
  - src/engine/languages/vue.rs
  - src/engine/linker.rs
  - tests/language_boundaries.rs
status: completed
receipt:
  passed_at: "2026-10-01T18:24:00Z"
  evidence:
    - "cargo test --test language_boundaries (10/10 passed)"
    - "vue_compiler_macros_are_setup_scoped_and_lexically_shadowable passes"
```

Recognizes Vue 3 `<script setup>` compiler macros (`defineProps`, `defineEmits`, `defineExpose`, `defineOptions`, `defineSlots`, `defineModel`, `withDefaults`, `ref`, `computed`, `reactive`, `watch`, `onMounted`, `useI18n`, `t`) and validates their admission within `<script setup>` ranges.

### task: python-mapping-and-logger-builtins

```yaml
task_ref: python-mapping-and-logger-builtins
target: "Support standard dictionary (dict) and logger (logging) methods on local Python variables"
proof_policy: seam-test-first
scope:
  - src/engine/languages/python.rs
  - src/engine/languages/python/value_flow.rs
  - src/engine/linker/semantic_context.rs
  - tests/python_semantics.rs
status: completed
receipt:
  passed_at: "2026-10-01T18:26:00Z"
  evidence:
    - "cargo test --test python_semantics (6/6 passed)"
    - "cargo test --test typed_receiver_resolution (54/54 passed)"
    - "tests/python_semantics.rs: local_receiver_dictionary_and_logger_methods_resolve_in_function_scope passes"
```

Recognizes standard dictionary methods (`row.get`, `data.items`, `keys`, `values`, `update`, `pop`) and logging methods (`logger.info`, `warning`, `debug`, `error`) across parameter-typed Mapping generics and local bindings.

### task: package-reexports-and-monorepo-hubs

```yaml
task_ref: package-reexports-and-monorepo-hubs
target: "Expand re-export chain resolution for monorepo internal packages"
proof_policy: seam-test-first
scope:
  - src/engine/languages/python.rs
  - src/engine/languages/python/fastmcp.rs
  - src/engine/languages/python/linker.rs
  - src/engine/languages/python/value_flow.rs
  - src/engine/linker.rs
  - src/engine/linker/receivers.rs
  - src/engine/linker/semantic_context.rs
  - tests/python_semantics.rs
status: completed
receipt:
  passed_at: "2026-10-01T19:15:21Z"
  evidence:
    - "cargo test --test python_semantics (7/7 passed)"
    - "cargo test --test fastmcp_registration (2/2 passed)"
    - "cargo test --test commitment_integrity (12/12 passed)"
    - "cargo clippy --all-targets --all-features -- -D warnings (0 warnings)"
    - "cargo test --all-targets (100% passed)"
    - "tests/python_semantics.rs: package_reexports_and_monorepo_hubs_resolve_overloads_type_aliases_and_runtime_implementations passes"
```

Resolve internal package re-export hubs (`contextunity.core.types`, `contextunity.commerce.catalogue.models`, and `contextunity.core.cell_edges`) so that exported types and aliases are tracked through package `__init__.py` files without dropping into unresolved or unverified external imports. Resolves overloaded decorator exports, simple PEP 484 type aliases, and runtime class definitions in `if TYPE_CHECKING: ... else: ...` constructs.

### task: typescript-npm-lockfile-and-workspace-externals

```yaml
task_ref: typescript-npm-lockfile-and-workspace-externals
target: "Extract external npm dependencies from package-lock.json, pnpm-lock.yaml, and yarn.lock for TS/JS"
proof_policy: seam-test-first
scope:
  - src/engine/languages/typescript.rs
  - src/engine/languages/manifests.rs
  - tests/typescript_semantics.rs
  - tests/manifests.rs
status: active
```

Parse `package-lock.json` (sections `packages` and `dependencies`), `pnpm-lock.yaml` (`dependencies`, `packages`), and `yarn.lock` in `typescript.rs:manifest_filenames` and `extract_manifest_dependencies`. Register all direct and transitive npm dependencies and scoped packages (`@types/*`, `@vue/*`, `axios`, etc.) as external origins, eliminating ~4,000 false `unresolved` symbols and elevating TypeScript resolution from 49% to >80%.

### task: javascript-node-globals-and-commonjs-resolution

```yaml
task_ref: javascript-node-globals-and-commonjs-resolution
target: "Expand Node.js/Web API builtins and support CommonJS require and module.exports"
proof_policy: seam-test-first
scope:
  - src/engine/languages/typescript.rs
  - src/engine/linker.rs
  - tests/typescript_semantics.rs
status: planned
```

Add standard Node.js runtime globals (`process.env`, `Buffer`, `path`, `fs`, `console`, `URL`, `setTimeout`) and Web API constructors (`fetch`, `Response`, `Request`, `Headers`). Resolve CommonJS module patterns (`const foo = require('./foo')`, `module.exports = { ... }`) to generate valid import/export graph edges, lifting JavaScript resolution from 30% to >60%.

### task: vue-sfc-script-setup-template-bridge

```yaml
task_ref: vue-sfc-script-setup-template-bridge
target: "Link local <script setup> scope with variables and components in <template> for .vue files"
proof_policy: seam-test-first
scope:
  - src/engine/languages/vue.rs
  - src/engine/ast/relations.rs
  - tests/language_boundaries.rs
status: planned
```

Bridge identifiers and component imports declared in `<script setup>` into the `<template>` AST evaluation scope. Ensure template expressions (`{{ count }}`, `@click="handler"`) and custom tags (`<UserCard />`) resolve to script bindings rather than leaking into unresolved symbols, lifting Vue coverage from 31% to >75%.

### task: html-django-template-tags-and-filter-builtins

```yaml
task_ref: html-django-template-tags-and-filter-builtins
target: "Add standard Django/Jinja tags and filters dictionary to HTML parser"
proof_policy: seam-test-first
scope:
  - src/engine/languages/html.rs
  - tests/html_profile.rs
status: planned
```

Recognize standard Django built-in template tags (`url`, `static`, `trans`, `block`, `include`, `extends`, `csrf_token`) and filter expressions (`|default`, `|date`, `|length`, `|json_script`, `|slugify`) in HTML templates, classifying them as framework builtins and preventing template directives from generating thousands of false unresolved function calls.
