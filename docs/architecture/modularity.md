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
│   ├── rust/               # Rust linker and value-flow facts
│   ├── typescript/         # TS/JS linker and CommonJS bindings
│   └── toml_manifest.rs    # TOML AST traversal shared by manifests
├── linker.rs               # Shared graph construction and dispatch
└── linker/
    ├── traits.rs           # LanguageLinker contract
    ├── semantic_context.rs # Shared bounded symbol admission
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

## Documentation Invariants

- **Rust Code**: Standard Rustdoc (`///` with Markdown, `# Arguments`, `# Returns`, `# Errors`, `# Panics`). Every public trait, struct, enum variant, and method must have self-contained documentation explaining ownership, invariants, and performance complexity.
- **Python Code**: Google-style docstrings (PEP 257) with strict section typing (`Args:`, `Returns:`, `Raises:`).
