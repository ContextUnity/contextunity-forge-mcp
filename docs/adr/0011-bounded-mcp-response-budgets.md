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
   - Enforce a default and maximum of 96 KiB (`max_output_bytes: 98304`) per MCP response. Configured smaller limits remain authoritative.
   - Count serialized UTF-8 content, response metadata, and JSON or Markdown framing within the response budget.
   - Establish default pagination limits of 30 items per response page.
2. **Generational Continuation Tokens**:
    - Return structured pagination metadata containing `next_offset`, `total`, and `generation`.
    - Validate code-map continuations against the indexed snapshot. Bind `task_list` continuations to the selected task workspace, milestone, filters, and task/subtask state instead; task-list generations are independent of code-index generations.
3. **Explicit Detail Levels**:
   - Expose lightweight symbol summaries by default.
   - Gate heavy evidence columns, syntax errors, and full docstring bodies behind explicit query parameters (`include_coverage`, `detail`).

## Consequences
- AI agents operate within bounded response sizes and paged result sets.
- MCP client context windows remain protected from payload flooding.
- Traversal operations remain bounded and responsive across large repositories.
