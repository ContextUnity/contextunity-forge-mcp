---
title: "ADR 0013: Data-Driven Framework Manifests"
doc_type: adr
status: superseded
date: 2026-10-05
---

# ADR 0013: Data-Driven Framework Manifests

## Status

Superseded for manifest file formats and parsing by [ADR 0017](0017-serde-framework-manifest-formats.md). Decisions 1, 2, 4, and 5 remain in force.

## Context

`LanguageLinker` is the language seam. Python, JavaScript, TypeScript, HTML, Vue, and Rust each keep their own linker implementation. Framework receivers, builtins, template filters, and route patterns that are not part of a language grammar currently sit in those language implementations. A second language-agnostic linker would duplicate that seam.

## Decision

1. **Language seam stays.** Framework manifests feed the existing `LanguageLinker` implementations. They do not replace `LanguageLinker` and they do not add a parallel linker.
2. **Universal grammar stays in the language profile.** Lexical scope, imports, and AST traversal remain in the language extractor. Vue stays a language profile in `src/engine/languages/vue.rs`. Nuxt directory conventions are a framework manifest consumed by the JavaScript and TypeScript linkers.
3. **Framework rules are data.** Receivers, builtins, filters, and route patterns that are not universal grammar live in a manifest. Standard manifests are `include_str!` tables in the binary. User manifest formats and parser policy are defined by [ADR 0017](0017-serde-framework-manifest-formats.md).
4. **Activation follows the dependency registry.** A manifest applies only when `DependencyRegistry` reports that the project declares that framework.
5. **Equivalence is status and endpoints.** A manifest migration preserves the multiset of resolution statuses and the edge triples `(src, dst, kind)` on the reference corpus. `commitment_integrity` stays deterministic. Evidence text may change, so the Merkle root is not required to match a pre-migration snapshot.

## Consequences

Language linkers consume framework data instead of growing new framework branches. A project that does not declare a framework does not receive that framework's receivers, filters, or routes. User manifests cost one validated read. Merkle leaves stay stable for a given evidence string and change when that string changes.
