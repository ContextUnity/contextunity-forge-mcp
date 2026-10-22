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
2. **Universal grammar stays in the language profile.** Lexical scope, imports, and AST traversal remain in the language extractor. Vue stays a language profile in `src/engine/languages/vue.rs`. Extractors, validators, RuleIR compilation, caches, and linkers implement finite typed operations without selecting behavior by framework identities or API spellings.
3. **Framework rules are data.** Receiver families, API and decorator identities, keyword and attribute names, directory conventions, activation conditions, and graph-relation mappings live in validated manifests. Standard manifests are `include_str!` tables in the binary; static bundle catalogues only bind identities to data files. User manifest formats and parser policy are defined by [ADR 0017](0017-serde-framework-manifest-formats.md). Pack tasks change data, tests, and documentation; missing engine capabilities belong to an admitted generic prerequisite.
4. **Activation follows the dependency registry.** Bundled legacy linker rules require a declared package. Workspace manifests are available only within their workspace. Effective typed extraction rules with empty activation retain their lexical, import, literal, and target proofs without requiring package metadata; explicit package alternatives use the nearest applicable ecosystem project. Semantic rules retain their declared project scope and activation.
5. **Equivalence is status and endpoints.** A manifest migration preserves the multiset of resolution statuses and the edge triples `(src, dst, kind)` on the reference corpus. `commitment_integrity` stays deterministic. Evidence text may change, so the Merkle root is not required to match a pre-migration snapshot.

## Consequences

Language linkers consume framework data instead of growing new framework branches. Removing or replacing a rule removes its framework relations while preserving ordinary language calls and imports. Renaming configured API, attribute, keyword, and directory spellings in both a manifest and a fixture preserves equivalent results without production changes. Activation follows the effective manifest rules. User manifests cost one validated read. Merkle leaves stay stable for a given evidence string and change when that string changes.
