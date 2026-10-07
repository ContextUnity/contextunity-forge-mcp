---
title: Universal AST search and path-scoped code navigation
doc_type: plan
purpose: >-
  Define a language-profile-driven AST search contract and path-aware, bounded
  MCP navigation so queries work consistently across current and future language
  profiles without duplicating parser wrappers or losing source matches to an
  unsound text prefilter.
depends_on: []
---

# Universal AST search and path-scoped code navigation

Admitted contract: [Milestone 042](../milestones/042-universal-ast-search-and-tool-navigation.md).

Keep grammar-specific fragment handling behind the extensible `LanguageProfile`
capability contract and the shared `src/engine/ast` search pipeline. Carry a caller's path
through every selector tool before ambiguity resolution, bound structured
ambiguity and preview responses, and keep search output useful enough to guide a
precise follow-up call. Reuse the existing indexed symbol metadata (`nodes.details`), source
access, and the sound AST fallback proven in milestone 030; do not add a raw-source
mirror or a second language-specific search implementation.

Navigation contracts build directly upon the completed text baseline in milestone 050,
promoting text-based ambiguity guidance to machine-readable JSON envelopes and
standardizing on `inspect_selector`.
