---
title: "ADR 0009: Manifest-Driven External Origin Classification"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0009: Manifest-Driven External Origin Classification

## Status
Accepted

## Context
Code bases depend on third-party libraries and standard libraries. Classifying unresolved third-party imports (e.g. `serde::Deserialize`, `requests.get`, `react`) as generic unresolved errors creates massive diagnostic noise, masks real project-internal link breakage, and blocks safe removal analysis.

## Decision
1. **Multi-Language Manifest Parsing**:
   - The scanner extracts dependency manifests across supported ecosystems:
     - Rust: `Cargo.toml`
     - Python: `pyproject.toml`, `requirements.txt`, `Pipfile`, `setup.cfg`
     - JavaScript/TypeScript: `package.json`
     - Go: `go.mod`
     - Java: `build.gradle`, `pom.xml`
2. **Dependency Registry**:
   - A shared `DependencyRegistry` aggregates known package names and ecosystem built-in namespaces.
3. **Four-Tier Resolution Status**:
   - Every reference receives one of four discrete statuses:
     - `resolved`: target symbol exists within indexed project workspace.
     - `external`: reference targets a package declared in a manifest or standard library, recording `external_origin`.
     - `ambiguous`: multiple workspace candidates match without narrowing evidence.
     - `unresolved`: reference belongs to workspace code but lacks a matching definition.

## Consequences
- Diagnostic and removal verification metrics isolate genuine project breakages from third-party imports.
- Removal analysis (`code_map_prove_removal`) evaluates true code health without false positives from external dependencies.
- External package usage is cleanly traceable through graph edges.
