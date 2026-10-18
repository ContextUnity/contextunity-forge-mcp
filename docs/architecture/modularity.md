---
title: "Language Engine Modularity and Extensibility Contracts"
doc_type: architecture
---

# Language Engine Modularity and Extensibility Contracts

## Architecture Overview

ContextUnity Forge MCP separates code parsing, AST fact extraction, cross-file symbol linking, and storage graph generation into modular, decoupled layers.

```text
src/engine/
├── ast/                    # Tree-sitter extraction and relation facts
├── languages/
│   ├── mod.rs              # LanguageProfile contract and linker registry
│   ├── support/template.rs # Byte-preserving TemplateMasker
│   ├── html.rs, vue.rs      # HTML islands and Vue template extraction
│   ├── python/             # Python linker and C3 receivers
│   ├── python/dependencies.rs # Python package dependency extraction
│   ├── manifests/          # Typed framework rule data
│   ├── dependency_registry.rs # Workspace dependencies and framework rules
│   ├── registries/         # JavaScript package and TypeScript path registries
│   ├── rust/               # Rust linker and value-flow facts
│   ├── typescript/         # TS/JS linker and CommonJS bindings
│   └── toml_manifest.rs    # TOML AST traversal shared by manifests
├── linker.rs               # Shared graph construction and dispatch
└── linker/
    ├── traits.rs           # LanguageLinker contract
    ├── semantic_context.rs # Shared bounded symbol admission
    ├── framework_relations.rs # Cross-language framework graph composition
    └── value_flow.rs       # Bounded static receiver reduction
```

## Extensibility Contracts

### 1. `LanguageProfile`
Encapsulates language-specific discovery and AST extraction:
- File extensions and filename patterns.
- Comment syntax and token boundary rules.
- Grammar definitions and Tree-sitter query conventions.
- Extraction of declarations (functions, methods, classes, types, interfaces).

### 2. `TemplateMasker`
Preprocesses mixed template syntax before the HTML or embedded JavaScript parser runs:
- Replaces `{% ... %}`, `{{ ... }}`, and `{# ... #}` bytes with spaces while retaining newline bytes and exact offsets.
- Reports original tag content and line to HTML extraction so `include` and `extends` remain structural references.
- Lets Vue extract interpolation references from original source spans between DOM nodes, including spans that the masked HTML parser omits as whitespace.
- Returns the borrowed source without a byte scan when no template marker is present.

### 3. `LanguageLinker`
Decouples language-specific import and receiver resolutions from the core SQLite graph builder:
- Import resolution (e.g. Python `__init__.py` re-export chains, TypeScript `tsconfig.json` path mapping, Rust `use crate::` paths).
- Receiver inference (method binding on class instances, struct instances, or prototypes).
- Standard library and external package classification, ensuring third-party symbols are properly marked `external_origin` rather than leaking into `unresolved`.
- Framework receivers, builtins, filters, and routes come from typed `FrameworkManifest` data. Bundled tables live under `src/engine/languages/manifests/` and are embedded in the binary. Legacy linker rules require a declared package; typed extraction rules retain their syntax and import checks without requiring package metadata. Workspace manifests load from `.forge/frameworks/` and are available in their workspace. They merge project rules before bundled rules by default or with `extend: true`; `replace: true` selects the project table alone. The four legacy string arrays retain their linker consumers. Typed AST rules compile into grammar-specific `RuleIR` candidates; class receivers retain their borrowed linker rule lookup, and `semantic_rules` facts join through the shared linker. Opaque legacy values acquire no generic execution semantics. See [Framework manifests](../reference/framework-manifests.md) for authoring and language-extension guidance.
- `DependencyRegistry` owns workspace dependency discovery and framework-manifest loading. JavaScript package metadata and TypeScript path mappings live in `languages/registries/`; Python dependency declarations are extracted in `python/dependencies.rs`. `LanguageProfile::manifest_filenames` continues to describe external package metadata inputs.

### 4. Three-Tier Language Architecture
Languages and extensions decompose into three explicit tiers:
- **Tier 1 (Declarative SyntaxGrammarSpec)**: Kind mappings for declarations, references, scopes, imports, member access and comments supply structural language support. Explicit unknown selectors fail through the parser error result; they never dispatch as kind zero.
- **Tier 2 (Single-Pass AST Extractor)**: `SinglePassExtractor` creates declaration identities on entry, accumulates body bindings and value flow during a shared DFS, and finalizes each owner on exit through a `ScopeFrame` stack. Nested owner frames retain their own summaries.
- **Tier 3 (Polyglot Semantic Linker)**: Deep language-specific member index construction, C3 inheritance resolution, and cross-layer framework relation composition.

The shared body traversal replaces recursive declaration-body binding, value-flow and final-return collectors. Occurrence-local conservative import and template-loader proofs may revisit lexical syntax. TypeScript's returned-object proof may inspect its enclosing function to establish same-object receiver metadata. These queries do not produce body summaries or replace scope-exit analysis. TypeScript module-alias preparation and DOM processing remain separate. This boundary does not claim that every parser operation visits every node only once. Tree-sitter query experiments remain opt-in research outside default indexing.

## Language resolution ownership

`semantic_context::LanguageMemberLookup` gives the shared resolver borrowed, read-only access to prebuilt Python, Rust, and TypeScript member indexes. Each language index retains its own lookup rules; the shared resolution input does not carry one field per language, and reference linking does not rebuild indexes or perform per-reference database reads.

`framework_relations::compose` joins route, HTTP request, HTMX, and HTML template facts after ordinary reference resolution. It consumes the indexed facts, owner set, flow state, dependency registry, language profiles, semantic context, and route nodes, then returns graph edges and coverage. The linker merges those results and applies its existing deterministic graph ordering.

## Bounded semantic resolution ownership

Python value flow currently joins direct identifier assignments from every explicit `if`/`elif`/`else` path. A missing `else` includes the incoming value; an unproven incoming value remains unknown. The shared reducer deduplicates by stable declaration identity, orders candidates deterministically, and widens the whole result when more than eight distinct local declarations are found. Other language-specific join syntax remains the responsibility of its language profile.

Exact receiver resolution and possible-target evidence remain separate outcomes. Candidate members use `possible_calls` with possible confidence and never become exact `calls` edges. Unknown alternatives remain incomplete; overflow past K=8 emits no truncated candidate prefix. These edges are persisted for navigation and included in inbound impact and removal assessments, while exact call counts continue to use only `calls`. The type-expression parser depth guard (eight), alias depth (default 64), reducer iteration limits, and candidate cardinality are independent bounds. These rules add no runtime evaluation and do not activate disabled language profiles.

## Documentation Invariants

- **Rust Code**: Standard Rustdoc (`///` with Markdown, `# Arguments`, `# Returns`, `# Errors`, `# Panics`). Every public trait, struct, enum variant, and method must have self-contained documentation explaining ownership, invariants, and performance complexity.
- **Python Code**: Google-style docstrings (PEP 257) with strict section typing (`Args:`, `Returns:`, `Raises:`).
