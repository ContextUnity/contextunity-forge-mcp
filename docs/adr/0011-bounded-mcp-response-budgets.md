---
title: "ADR 0011: Bounded MCP Response Budgets and Continuation Paging"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0011: Bounded MCP Response Budgets and Continuation Paging

## Status
Accepted

## Context
AI coding assistants consume finite LLM context windows. Unbounded graph traversals or massive symbol query payloads exhaust context tokens and introduce latency spikes.

## Decision
1. **Response Output Limits**:
   - Enforce an upper ceiling of 64 KiB (`max_output_bytes: 65536`) per MCP response.
   - Establish default pagination limits of 30 items per response page.
2. **Generational Continuation Tokens**:
   - Return structured pagination metadata containing `next_offset`, `total`, and `generation`.
   - Validate continuation requests against the current indexed snapshot token so a page cannot silently continue against a replaced database snapshot.
3. **Explicit Detail Levels**:
   - Expose lightweight symbol summaries by default.
   - Gate heavy evidence columns, syntax errors, and full docstring bodies behind explicit query parameters (`include_coverage`, `detail`).

## Consequences
- AI agents operate within bounded response sizes and paged result sets.
- MCP client context windows remain protected from payload flooding.
- Traversal operations remain bounded and responsive across large repositories.
