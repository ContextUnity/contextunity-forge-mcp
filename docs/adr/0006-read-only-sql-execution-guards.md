---
title: "ADR 0006: Read-Only SQL Execution Sandbox and Injection Guards"
doc_type: adr
status: accepted
date: 2026-10-01
---

# ADR 0006: Read-Only SQL Execution Sandbox and Injection Guards

## Status
Accepted

## Context
Tools like `code_map_query(operation="sql")` and `code_map_analyze` accept SQL queries from AI agents to enable custom diagnostic queries. Untrusted LLM-generated SQL queries risk executing mutations, dropping tables, running administrative statements, or chaining stacked injection payloads.

## Decision
1. **SQLite Read-Only Enforcement**:
   - Connection handles for user queries set `PRAGMA query_only = ON;`.
   - Read-only transactions isolate SQL queries from modifying table contents or database schema.
2. **AST Statement Validation & Keyword Rejection**:
   - Query inputs are parsed to verify they comprise a single `SELECT` or `WITH` statement.
   - Any statement containing mutation keywords (`INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `CREATE`, `ATTACH`, `DETACH`, `VACUUM`, `REINDEX`) is rejected before execution.
3. **Stacked Statement Prevention**:
   - Statements with semicolons separating multiple commands are rejected to eliminate SQL chaining attacks.
4. **Execution Budgets**:
   - Queries execute under strict execution time limits (5000 ms) and row return limits (default 30, maximum 100 rows).

## Consequences
- AI agents safely run diagnostic SQL without risk of database corruption or privilege escalation.
- Query latencies remain bounded by execution timeouts and pagination limits.
- SQLite graph data remains immutable to all external query tools.
