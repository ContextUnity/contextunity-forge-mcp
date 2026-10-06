---
title: "ADR 0008: Bounded Static Value Flow and Receiver Resolution"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0008: Bounded Static Value Flow and Receiver Resolution

## Status
Accepted

## Context
Accurate method call linking requires receiver type inference (e.g. knowing that `user.save()` calls `User.save`). Dynamic evaluation (e.g. running code in an interpreter) introduces arbitrary execution risks and severe latency overheads. Unbounded static analysis risks infinite loops on cyclic alias chains or path explosions.

## Decision
1. **Purely Static Data-Flow Analysis**:
   - Infer receiver types through lexical AST facts: assignment positions, explicit type annotations, constructor invocations, and straight-line factory returns.
   - Prohibit runtime evaluation or process sandboxing.
2. **Deterministic Termination & Bounded Traversal**:
   - Traversal depth for straight-line return chains is capped to 4 hops.
   - Variable reassignments, conditional branches (`if/else`), and cyclic aliases terminate type inference immediately, falling back to unresolved status or opaque dispatch.
3. **No Interprocedural Side-Effect Speculation**:
   - Receiver inference operates within proven lexical scope boundaries. Global or mutated variables without deterministic local initialization remain unresolved.

## Consequences
- Deterministic linker execution through bounded, fail-closed traversal.
- Zero security risk from executing arbitrary codebase scripts.
- Confident call graph edges with explicit provenance for resolved members.
