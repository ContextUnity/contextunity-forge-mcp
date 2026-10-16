---
title: "ADR 0017: Serde-Driven Framework Manifest Formats"
doc_type: adr
status: accepted
date: 2026-10-07
supersedes: 0013
---

# ADR 0017: Serde-Driven Framework Manifest Formats

## Status

Accepted by the milestone owner to resolve `ARCH-BLOCKER-041-TOML-SCOPE-COLLISION`. This ADR supersedes Decision 3's user-file format and parser policy in [ADR 0013](0013-data-driven-framework-manifests.md); its decisions on the `LanguageLinker` seam, grammar boundary, bundled activation, and migration equivalence remain in force.

## Context

The custom Tree-sitter walk in the framework manifest loader duplicated TOML semantic validation and accepted dotted key/table collisions across nested scopes. Framework files are configuration documents, so their format semantics belong to dedicated TOML and YAML deserializers. The affected architecture is described in [Engine Modularity and Traits](../architecture/modularity.md), and manifest activation remains governed by the language dependency registry.

## Decision

1. Read user framework manifests from `.forge/frameworks/` with `.toml`, `.yaml`, or `.yml` extensions. Use the file stem as the framework identifier.
2. Parse TOML with `toml = "0.8"` and YAML with the existing `serde_yaml = "0.9"` dependency, deserializing both formats into the same serde-derived typed manifest.
3. Require `receivers`, `builtins`, `filters`, and `routes` to be arrays. Reject missing sections, non-array values, malformed input, unknown top-level sections, and all parser errors before index publication.
4. Preserve the existing `LanguageLinker` seam, language-owned grammar and lexical rules, and equivalence requirements from ADR 0013. Activate workspace framework manifests placed in `.forge/frameworks/<id>.toml`, `.yaml`, or `.yml` unconditionally for that workspace. Merge project rules before matching bundled rules by default or with `extend: true`; `replace: true` uses only project rules. Reject manifests that set both flags to true. Activate bundled manifests only when `DependencyRegistry` reports that the package is declared in the source file's workspace.

## Consequences

Users can express the same typed framework rules in TOML or YAML. TOML key, table, and dotted-key semantics are validated by the TOML deserializer; YAML syntax and typed-shape errors are validated by the YAML deserializer. The four rule arrays stay required and unknown top-level fields remain errors. Project-first merging preserves opaque nested rule values without claiming that the linker executes them. Any invalid user file fails the build before an index is published.
