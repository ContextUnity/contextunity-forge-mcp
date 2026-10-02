---
title: "Language Engine Modularity and Extensibility Contracts"
doc_type: architecture
---

# Language Engine Modularity and Extensibility Contracts

## Architecture Overview

ContextUnity Forge MCP separates code parsing, AST fact extraction, cross-file symbol linking, and storage graph generation into modular, decoupled layers.

```text
src/engine/
├── parser/
│   ├── traits.rs           # LanguageParser & TemplatePreprocessor traits
│   ├── sanitize.rs         # Byte-offset-preserving template tag sanitizer ({% %}, {{ }})
│   └── tree.rs             # Safe Tree-sitter traversal, error collection
├── languages/
│   ├── profile.rs          # LanguageProfile trait (extensions, keywords, builtins)
│   ├── python/             # Python facts extractor, AST visitor, re-exports
│   ├── web/                # HTML, Vue, Django template resolution
│   ├── rust/               # Rust facts extractor, module hierarchy
│   └── typescript/         # TS/JS facts extractor, path aliases
└── linker/
    ├── traits.rs           # LanguageLinker trait (resolve_import, resolve_receiver)
    ├── engine.rs           # Universal graph resolution engine
    └── profiles/           # Language-specific linker profiles (python.rs, etc.)
```

## Extensibility Contracts

### 1. `LanguageProfile`
Encapsulates language-specific discovery and AST extraction:
- File extensions and filename patterns.
- Comment syntax and token boundary rules.
- Grammar definitions and Tree-sitter query conventions.
- Extraction of declarations (functions, methods, classes, types, interfaces).

### 2. `TemplatePreprocessor`
Preprocesses mixed template syntaxes (e.g. Django/Jinja tags inside HTML or Vue templates) before Tree-sitter parsing:
- Replaces template delimiters (`{% ... %}`, `{{ ... }}`, `{# ... #}`) with space characters matching exact byte lengths.
- Preserves exact byte offsets and line numbering, preventing Tree-sitter syntax errors while retaining faithful spans for code navigation.
- Directly extracts template references (`include`, `extends`, `component`) into structural fact relationships.

### 3. `LanguageLinker`
Decouples language-specific import and receiver resolutions from the core SQLite graph builder:
- Import resolution (e.g. Python `__init__.py` re-export chains, TypeScript `tsconfig.json` path mapping, Rust `use crate::` paths).
- Receiver inference (method binding on class instances, struct instances, or prototypes).
- Standard library and external package classification, ensuring third-party symbols are properly marked `external_origin` rather than leaking into `unresolved`.

## Documentation Invariants

- **Rust Code**: Standard Rustdoc (`///` with Markdown, `# Arguments`, `# Returns`, `# Errors`, `# Panics`). Every public trait, struct, enum variant, and method must have self-contained documentation explaining ownership, invariants, and performance complexity.
- **Python Code**: Google-style docstrings (PEP 257) with strict section typing (`Args:`, `Returns:`, `Raises:`).
