---
title: "Forge MCP: Architecture, Modularity, and Quality Roadmap"
repository: forge-mcp
area: forge
priority: high
status: active
kind: plan
source_path: docs/plans/architecture-and-modularity.md
related_plans:
  - docs/plans/tool-performance-and-db-optimization.md
doc_type: guide
related_milestones:
  - docs/milestones/010-repository-task-lifecycle.md
---

# ContextUnity Forge MCP — Architecture, Modularity, and Quality Roadmap

## 1. Document Architecture & Standards

### 1.1 Documentation & Docstring Invariants
- **Rust Standard:** Standard Rustdoc (`///` with Markdown, `# Arguments`, `# Returns`, `# Errors`, `# Panics`, `# Examples`). Every public trait, struct, enum variant, and method must have self-contained documentation explaining ownership, thread-safety, invariants, and performance complexity.
- **Python Standard:** Google-style docstrings (PEP 257) with strict section typing (`Args:`, `Returns:`, `Raises:`, `Yields:`).
- **Architecture Documentation:** Stored under `docs/architecture/` in repository:
  - `languages.md`: Rules for implementing `LanguageProfile`, Tree-sitter query conventions, AST traversal.
  - `linking.md`: Cross-file symbol resolution, re-exports, provenance, dynamic receivers.
  - `concurrency.md`: SQLite locks, generation counters, connection slots, read/write guarantees.

---

## 2. Target System Architecture & Modularity

### 2.1 Language System Layering
```text
src/engine/
├── parser/
│   ├── traits.rs           # LanguageParser & TemplatePreprocessor traits
│   ├── sanitize.rs         # Byte-offset-preserving template tag sanitizer ({% %}, {{ }})
│   └── tree.rs             # Safe Tree-sitter traversal, error collection
├── languages/
│   ├── profile.rs          # LanguageProfile trait (extensions, keywords, builtins)
│   ├── python/             # Python facts extractor, AST visitor, re-exports
│   ├── web/                # HTML, Vue, Template include/extends resolution
│   ├── rust/               # Rust facts extractor, module hierarchy
│   └── typescript/         # TS/JS facts extractor, path aliases
└── linker/
    ├── traits.rs           # LanguageLinker trait (resolve_import, resolve_receiver)
    ├── engine.rs           # Universal graph resolution engine
    └── profiles/           # Language-specific linker profiles (python.rs, etc.)
```

### 2.2 Extensibility Contracts
- `LanguageProfile`: Encapsulates file discovery, comment syntax, declaration models.
- `TemplatePreprocessor`: Replaces template delimiters (`{% ... %}`, `{{ ... }}`) with whitespace preserving byte lengths before handing source to tree-sitter, avoiding syntax errors while retaining exact byte/line mappings.
- `LanguageLinker`: Decouples language-specific import resolutions (e.g. Python package `__init__.py` chains, TypeScript `tsconfig.json` paths, Rust `use crate::`) from the core SQLite graph builder.

---

## 3. Work Breakdown & Execution Plan

### Stage 1: Storage, Concurrency & Release Hygiene [COMPLETED]
- [x] Protect SQLite generations across Forge readers via `generation_lock` and cache slot validation.
- [x] Eliminate `/proc/*/fd` directory scans from cache invalidation.
- [x] Preserve MCP SQLite connection caching in `src/mcp/server.rs` with RAII reader locks.
- [x] Set SQLite pragmas (`PRAGMA mmap_size = 268435456;`, `PRAGMA synchronous = NORMAL;`).
- [x] Fix Clippy warnings and format code with zero regressions.
- [x] Repack dirty commits into clean, semantic history (`0b36623` .. `5bbaefa`).

### Stage 2: HTML/Django Template Pre-lexer & Parser Sanitizer [COMPLETED]
- [x] Implement template pre-processing in `src/engine/languages/html.rs`:
  - Identify Django/Jinja tags `{% ... %}`, variables `{{ ... }}`, and comments `{# ... #}`.
  - Mask them with whitespace preserving exact byte lengths so Tree-sitter HTML parser parses clean DOM.
  - Extract template linkages (`{% include "..." %}`, `{% extends "..." %}`) directly into file reference facts.
- [x] Eliminate HTML parse errors on Django templates (verified in `tests/html_profile.rs`).

### Stage 3: Vue Template Class & Token Extraction Fix [COMPLETED]
- [x] In `src/engine/languages/vue.rs`, implement `strip_string_literals` to prevent CSS class token splitting (e.g. `bg-white/5` inside quotes) from polluting global symbols (verified in `tests/language_boundaries.rs`).

### Stage 4: Search & Navigation UX from Codebase Memory MCP (CBM)
- [ ] Implement `grouped_by_file` option in symbol/code search results to optimize LLM context consumption.
- [ ] Aggregate error diagnostics into line-ranges instead of generating dozens of separate syntax error nodes.
- [ ] Implement comprehensive performance hardening & SQLite database compaction as specified in [`tool-performance-and-db-optimization.md`](tool-performance-and-db-optimization.md):
  - Localize `code_map_prove_removal` to target dependencies (unblocking safe removal).
  - Two-tier hybrid ranking with native SQLite FTS5 BM25 in `code_map_search` and debounced MCP admission inventory scans (<30ms).
  - Bounded traversal (`max_depth = 4`) and lexical fallback in `code_map_tests`.
  - Database compaction from 839 MiB to <200 MiB (Zstandard facts blob, deduplicating `reverse_dependencies`, dictionary evidence).

### Stage 5: Modularity Refactoring & Language Traits
- [ ] Extract `LanguageProfile` and `LanguageLinker` traits.
- [ ] Move `src/engine/linker/python.rs` to clean modular structure under `src/engine/languages/python/linker.rs`.
- [ ] Add comprehensive Rustdoc documentation to all engine modules.

### Stage 6: Codex Peer Review & Gate Verification
- [ ] Invoke headless `codex review` / `codex exec` to conduct rigorous peer review of all staged changes.
- [ ] Verify test suite passes (`cargo test --all-targets`).
