---
title: "ADR 0007: Fail-Closed Selector Disambiguation Pipeline"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0007: Fail-Closed Selector Disambiguation Pipeline

## Status
Accepted

## Context
AI agents and developers supply symbol selectors in varied formats (bare function names, module paths, file lines). In large codebases, identical identifiers exist across unrelated packages (e.g. `parse`, `validate`, `Config`). Heuristic guessing risks semantic hijacking—evaluating impact or removal safety against the wrong symbol.

## Decision
1. **Four-Tier Resolution Pipeline**:
   - Tier 1: Exact symbol ID (e.g. `def:src/auth.py:parse:14`).
   - Tier 2: Qualified name (e.g. `auth::Parser.parse`).
   - Tier 3: Path-scoped coordinates:
     - Symbol within file: `src/auth.py:parse` or `src/auth.py::parse`.
     - Narrowest symbol covering line: `src/auth.py:14` or `src/auth.py#L14`.
   - Tier 4: Bare symbol name (e.g. `parse`).
2. **Fail-Closed Ambiguity Defense**:
   - When a bare or partially qualified selector matches multiple distinct symbols, the resolver halts immediately and raises `AMBIGUOUS_SELECTOR`.
   - The error response enumerates matching candidate IDs and file paths, requiring the caller to supply a narrowed selector.

## Consequences
- Operations like `code_map_impact` and `code_map_prove_removal` execute exclusively against verified single targets.
- Semantic hijacking across homonymous symbols is eliminated.
- Callers receive actionable narrowing options on ambiguous inputs.
