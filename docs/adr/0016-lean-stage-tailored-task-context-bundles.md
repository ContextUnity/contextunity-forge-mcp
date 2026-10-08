---
title: "ADR 0016: Lean Stage-Tailored Task Context Bundles and Response Bounding"
doc_type: adr
status: accepted
date: 2026-10-07
---

# ADR 0016: Lean Stage-Tailored Task Context Bundles and Response Bounding

## Status

Accepted.

## Context

ADR 0011 established an upper ceiling of 64 KiB (`max_output_bytes: 65536`) per MCP response to protect LLM context windows and prevent latency spikes. ADR 0015 introduced zero-shot context bundle aggregation by default upon `task claim` and `task context`.

However, in multi-revision tasks or mature milestones, `task_claim` responses began exceeding the 64 KiB ceiling and failing closed with `Output exceeds the response byte limit` (e.g. reaching 82 KiB in tasks with multiple historical gate attempts). Root cause analysis revealed four structural flaws:
1. **Raw Historical Archive Dumps**: The underlying `inspect_details` payload dumped entire SQLite tables into the response envelope: `gates` (containing raw multiline test execution outputs across all historical revisions, alone consuming 18+ KiB), `findings` (historical review findings), and `attempts` (all past claim attempts).
2. **Root Key Duplication**: Four major sub-structures (`adrs`, `scope_symbols`, `covering_tests`, `blackboard`) were serialized twice—once inside `context_bundle` and a second time directly at the root of the JSON response, producing 12+ KiB of pure redundancy.
3. **Unbounded Blackboard Payloads**: Blackboard queries pulled up to 50 historical messages from the milestone hierarchy with raw, unconstrained payloads (consuming 8+ KiB).
4. **Stage-Agnostic Symbol Projections**: Reviewers and delivery workers received identical raw symbol tables and test lists as contract authors, despite only requiring candidate diff snapshots (`inspect_cmd`) or milestone delivery metadata.

## Decision

1. **Stage-Differentiated Bundle Generation**:
   The `context_bundle` payload is tailored dynamically to the active ACDD stage:
   - **`contract/v1`**: Delivers `contract` (target, allowed_write_scope, subtasks, depends_on, invariants, proof_policy), `guidance` (seam proof and subtask acceptance), `adrs` (governing architectural decisions), `scope_symbols` (indexed symbol skeleton), `covering_tests` (scoped test suites), and `blackboard`.
   - **`build/v1`**: Delivers `contract` (with `depends_on`), `guidance` (acceptance and implementation steps), `adrs`, `scope_symbols`, `covering_tests`, `contract_seam_test` (the approved seam test ref/command extracted from the passed `contract/v1` gate), `unresolved_review_findings` (structured JSON findings present strictly when the most recent `review/v1` or `deliver/v1` gate attempt was rejected), and `blackboard`.
   - **`review/v1`**: Delivers `contract` (scope and invariants to verify against the diff), `guidance` (5 review contours and review policy), `adrs`, `candidate_snapshot` (the commit hash and `git show` inspect command of the candidate from the latest passed `build/v1` gate; omitted if build passed without a commit), and `blackboard`. Omit symbol skeletons and test suite lists.
   - **`deliver/v1` / `completed`**: Delivers `contract`, `guidance` (atomic commit rules), `latest_snapshot`, `milestone_ref`, `receipt` (inside `context_bundle` strictly when status is `completed`), and `blackboard`. Omit symbols, tests, and ADR lists.

2. **Historical Table Pruning from Context Operations and Minimal Claim**:
   - `task context` and default `task claim` prune raw database dumps (`gates`, `attempts`, `findings`, raw `spec`, and root `receipt`) from the returned envelope, while explicitly preserving `depends_on` in `contract` and at envelope root, and preserving completion `receipt` inside `context_bundle` for completed tasks. Detailed gate history and previous test logs remain fully accessible on demand through `task_manage(action: "inspect")` or CLI `task inspect <id>`.
   - Minimal claim mode (`bundle: false`): Delivers lean task metadata (`task_id`, `stage`, `status`, `allowed_write_scope`, `subtasks`, `depends_on`, `workflow_guidance`) strictly pruned of heavy `gates`, `attempts`, `findings`, `receipt`, and raw `spec` archives, ensuring minimal mode never exhausts response limits.

3. **Elimination of Root Key Duplication**:
   Remove root duplicates of `adrs`, `scope_symbols`, and `covering_tests`. Retain only `blackboard` and `depends_on` at the envelope root alongside `context_bundle` for backward compatibility with existing consumers.

4. **Bounded Blackboard and Symbol Skeletons**:
   - `blackboard` queries aggregate across the complete milestone hierarchy: task-scoped messages (`task_id`), parent milestone-level messages (`milestone_ref` where `task_id IS NULL`), and sibling tasks in the same milestone, bounded to the 15 most recent chronological messages (`ORDER BY created_at DESC, id DESC LIMIT 15` reversed). Every message is explicitly annotated with its origin scope (`scope: "milestone" | "task" | "subtask" | "sibling"`), `task_id`, and `subtask_ref`.
   - Individual blackboard message payloads exceeding 500 Unicode characters and symbol signatures exceeding 200 Unicode characters are safely truncated along character boundaries (never slicing byte boundaries). Full message payloads remain accessible via `task_blackboard(action: "inspect", message_id: <id>)`.

## Consequences

- Task claim and context responses shrink from > 82 KiB to < 21 KiB (over 74% reduction), remaining safely within the 64 KiB ceiling of ADR 0011 across all stages and claim revisions.
- Minimal claim mode (`bundle: false`) is truly minimal and immune to historical gate log bloat.
- UTF-8 text truncation is completely character-safe across international text (Ukrainian, CJK, emoji), eliminating panics that previously left tasks trapped in `in_progress`.
- Task dependencies (`depends_on`) remain observable across all claims and context views.
- LLM agents receive stage-relevant operational guidance and exact references (e.g. `contract_seam_test` on build, `candidate_snapshot` on review) without token-wasting noise.
- Historical gate logs and full review archives remain preserved in SQLite and accessible via `task inspect`.
