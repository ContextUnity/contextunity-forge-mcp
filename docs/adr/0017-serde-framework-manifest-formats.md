---
title: "ADR 0017: Serde-Driven Framework Manifest Formats"
doc_type: adr
status: accepted
date: 2026-10-07
supersedes: 0013
---

# ADR 0017: Serde-Driven Framework Manifest Formats

## Status

Accepted. Decision 3 defines the current typed manifest schema and supersedes the format and parser policy in [ADR 0013](0013-data-driven-framework-manifests.md). ADR 0013 continues to define the linker and grammar boundaries.

## Context

Framework files are typed configuration documents. TOML and YAML deserializers validate their format semantics, while `FrameworkManifest` restricts accepted rule data to supported typed sections. Manifest activation remains governed by the language dependency registry.

## Decision

1. Read user framework manifests from `.forge/frameworks/` with `.toml`, `.yaml`, or `.yml` extensions. Use the file stem as the framework identifier.
2. Parse TOML with `toml = "0.8"` and YAML with the existing `serde_yaml = "0.9"` dependency, deserializing both formats into the same serde-derived typed manifest.
3. Deserialize manifests into the closed typed schema. Rule sections are optional; supported class, route, dependency-injection, semantic, and syntax sections use typed records and closed serde operation variants with strict unknown-field rejection and operation-specific validation. Reject top-level `receivers`, `builtins`, `filters`, and `routes` lists, malformed input, unknown top-level sections, and all parser errors before index publication. API identities, keyword and attribute names, directory conventions, activation, and finite graph operations are data; arbitrary executable extraction or query strings are unsupported. Project rules merge by pack-local rule identity before language preparation and grammar dispatch.
4. Preserve the existing `LanguageLinker` seam and language-owned grammar and lexical rules. Make workspace framework manifests placed in `.forge/frameworks/<id>.toml`, `.yaml`, or `.yml` available within that workspace. Merge project rules before matching bundled rules by default or with `extend: true`; `replace: true` uses only project rules. Reject manifests that set both flags to true. Rules with package activation require a declaration in the nearest applicable ecosystem project. Rules without package activation still require their configured syntax, import, literal, and target proofs; semantic rules retain their declared project scope and activation.

## Consequences

Users can express the same typed framework rules in TOML or YAML. TOML key, table, and dotted-key semantics are validated by the TOML deserializer; YAML syntax and typed-shape errors are validated by the YAML deserializer. Rule sections are optional, and unknown top-level fields—including the four unsupported raw string lists—remain errors. Any invalid user file fails the build before an index is published.
