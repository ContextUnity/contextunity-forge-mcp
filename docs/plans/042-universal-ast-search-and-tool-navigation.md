---
title: Universal AST search and path-scoped code navigation
doc_type: plan
purpose: >-
  Define a language-profile-driven AST search contract and path-aware, bounded
  MCP navigation so queries work consistently across current and future language
  profiles without duplicating parser wrappers or losing source matches to an
  unsound text prefilter.
depends_on:
  - m-language-semantics-and-resolution-coverage
  - m-tool-performance-and-storage-compaction
  - m-framework-manifests-and-parser-modularity
---

# Universal AST search and path-scoped code navigation

Admitted contract: [Milestone 042](../milestones/042-universal-ast-search-and-tool-navigation.md).

Keep grammar-specific fragment handling behind the extensible `LanguageProfile`
contract and the shared `src/engine/ast` search pipeline. Carry a caller's path
through every selector tool before ambiguity resolution, bound structured
ambiguity and preview responses, and keep search output useful enough to guide a
precise follow-up call. Reuse the existing indexed symbol metadata, source
access, and the separate milestone 030 body-token plan; do not add a raw-source
mirror or a second language-specific search implementation.

The follow-on text-tool milestone consumes these stable response contracts. The
optical-token hypothesis in milestone 051 continues to compare the final text
surface from milestone 050 and does not add image transport for source, diffs, or
signatures.
