---
id: m-architecture-and-modularity
title: "Architecture, modularity, and language trait decoupling"
doc_type: contract
status: planned
depends_on:
  - m-language-semantics-and-resolution-coverage:completed
owners:
  - src/engine/
  - src/core/
  - tests/
invariants:
  - "INV-SEPARATE-CONCERNS: Language extractors and language linkers are encapsulated behind typed traits."
  - "INV-BYTE-PRESERVING: Template preprocessors preserve byte offsets and character positions without syntax mangling."
related_plans:
  - docs/plans/architecture-and-modularity.md
---

# Architecture, modularity, and language trait decoupling

## Outcome and purpose

Decouple monolithic linker logic in `src/engine/linker.rs` into modular per-language linkers implementing the `LanguageLinker` trait. Generalize HTML/Vue/Django template preprocessing into byte-offset-preserving sanitizers. Ensure all public traits, structs, and interfaces have complete Rustdoc documentation.

## Tasks in this milestone

### task: language-linker-traits-decoupling

```yaml
task_ref: language-linker-traits-decoupling
target: "Винести специфічну логіку лінкування з linker.rs у модульні профілі за трейтом LanguageLinker"
proof_policy: seam-test-first
scope:
  - src/engine/linker/traits.rs
  - src/engine/linker.rs
  - src/engine/languages/python/linker.rs
  - src/engine/languages/rust/linker.rs
  - src/engine/languages/typescript/linker.rs
  - tests/
status: planned
```

Refactor `src/engine/linker.rs` by extracting language-specific import and receiver dispatch into modular implementations of `LanguageLinker`. Move Python-specific, Rust-specific, and TypeScript-specific resolution helpers into their respective language modules while keeping the central graph traversal engine clean and language-agnostic.

### task: template-preprocessor-generalization

```yaml
task_ref: template-preprocessor-generalization
target: "Уніфікувати препроцесори шаблонів Django/Jinja, Vue та HTML зі збереженням точних байтових зміщень"
proof_policy: seam-test-first
scope:
  - src/engine/languages/html.rs
  - src/engine/languages/vue.rs
  - tests/html_profile.rs
status: planned
```

Generalize template island sanitizers so that delimiters (`{% ... %}`, `{{ ... }}`, `<script ...>`) are processed through a shared, byte-offset-preserving tokenizer. Retain exact byte and line coordinates for downstream Tree-sitter parsers while avoiding DOM syntax errors on template syntax.

### task: public-rustdoc-completeness

```yaml
task_ref: public-rustdoc-completeness
target: "Покрити всі публічні трейти, структури та методи вичерпною документацією Rustdoc"
proof_policy: seam-test-first
scope:
  - src/core/
  - src/engine/
  - src/db/
  - src/mcp/
status: planned
```

Ensure standard Rustdoc (`///` with Markdown, `# Arguments`, `# Returns`, `# Errors`, `# Panics`) across every public trait, struct, enum variant, and method in `src/`, documenting thread-safety, ownership models, invariants, and performance complexity.
