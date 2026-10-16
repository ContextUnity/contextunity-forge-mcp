---
id: plan-053-declarative-tree-sitter-queries-study
title: "Research Study: Declarative Tree-sitter Queries (.scm) for Language Onboarding"
doc_type: plan
status: proposed
date: 2026-10-16
related_milestones:
- m-language-parsers-and-declarative-framework-packs
---

# Research Study: Declarative Tree-sitter Queries (`.scm`) for Language Onboarding

## 1. Executive Summary

ContextUnity Forge MCP currently implements language extraction via custom, imperative Rust AST walkers for each language (e.g. `src/engine/languages/rust.rs`, `python.rs`, `typescript/mod.rs`, `vue.rs`). Adding a new programming language (e.g., Go, C#, Kotlin, PHP, Ruby, SQL) currently requires writing hundreds of lines of imperative Rust code manipulating tree cursors and node kinds.

This research study evaluates introducing **Declarative Tree-sitter Queries (`.scm`)** as an **optional, opt-in extraction mode** (`--features declarative-scm` or configuration flag `engine.declarative_queries = true`).

---

## 2. Architecture of Tree-sitter S-Expression Queries

Tree-sitter provides a native C query engine that compiles S-expression patterns into deterministic finite-state automata (DFAs).

### Example: Declarative Symbol Extraction (`queries/typescript/symbols.scm`)

```scheme
;; Extract function declarations
(function_declaration
  name: (identifier) @symbol.name
  parameters: (formal_parameters) @symbol.signature
  body: (statement_block) @symbol.body
) @symbol.function

;; Extract class declarations and inheritance
(class_declaration
  name: (type_identifier) @symbol.name
  (class_heritage
    (extends_clause value: (identifier) @symbol.extends))?
) @symbol.class

;; Extract call expressions
(call_expression
  function: [
    (identifier) @call.target
    (member_expression property: (property_identifier) @call.target)
  ]
  arguments: (arguments) @call.args
) @call
```

### Capture Mapping to Forge Graph

The capture names map directly to normalized Forge graph primitives:
- `@symbol.<kind>`: Emits `Node` with corresponding `NodeKind` (Function, Class, Method, Interface).
- `@symbol.name`: Resolves the identifier token and text span.
- `@call.target`: Emits `Fact::Call { target, span }`.
- `@import.source`: Emits `Fact::Import { source, specifiers }`.

---

## 3. Opt-in Feature Design

To ensure zero risk to existing language extractors:
1. **Compilation Gate**: Gated behind `cfg(feature = "declarative-scm")`. The default release binary retains the proven imperative extractors.
2. **Runtime Fallback**: When enabled, declarative queries handle basic Tier 1 languages (e.g., Go, C#), while deep Tier 2/Tier 3 languages (Python, TypeScript, Rust) retain native semantic linkers.
3. **Hot-Reloadable Queries**: For rapid developer iteration, `.scm` query files can be read from `.forge/queries/<language>/` without recompiling Forge MCP.

---

## 4. Trade-Off Analysis

| Dimension | Imperative Rust Extractor | Declarative `.scm` Queries |
|---|---|---|
| **Authoring Speed** | Slow (~500–1500 LOC per language) | Fast (50–100 lines of `.scm` queries) |
| **New Language Onboarding** | Requires Forge MCP recompilation | Drop `.scm` file + grammar dylib |
| **Extraction Throughput** | Optimized, direct struct instantiation | Tree-sitter C DFA matching (~similar, but query cursor allocation) |
| **Complex Value Flow** | Native support (state machines, scopes) | Limited (requires post-processing hooks) |
| **Maintenance Burden** | High Rust code footprint | High declarative query footprint |

---

## 5. Experimental Verification Plan

1. **Benchmark Seam**:
   - Create a benchmark corpus comparing imperative TypeScript extraction against `.scm` query extraction across 5,000 source files.
   - Measure: throughput (files/sec), peak memory, and capture count parity.
2. **Cold Indexing Invariant**:
   - The declarative engine is admitted only if cold indexing throughput is $\ge 95\%$ of the hand-written extractor on equivalent grammar trees.
3. **Prototype Delivery**:
   - Prototype `.scm` extraction for a new lightweight language (e.g. Go or SQL) without writing Rust AST traversal logic.
