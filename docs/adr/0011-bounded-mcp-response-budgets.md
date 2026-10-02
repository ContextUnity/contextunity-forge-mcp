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
   - Return structured pagination metadata containing `next_offset`, `total_count`, and `generation`.
   - Validate client continuation requests against the active database generation counter to prevent reading across concurrent database rebuilds.
3. **Explicit Detail Levels**:
   - Expose lightweight symbol summaries by default.
   - Gate heavy evidence columns, syntax errors, and full docstring bodies behind explicit query parameters (`include_coverage`, `detail`).

## Consequences
- AI agents operate within predictable token envelopes and low response latencies.
- MCP client context windows remain protected from payload flooding.
- Traversal operations remain bounded and responsive across large repositories.
