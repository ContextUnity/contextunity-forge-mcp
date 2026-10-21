---
title: "ADR 0019: Markdown-First Dual Surface Output and Dead Metadata Compaction"
doc_type: adr
status: accepted
date: 2026-10-21
---

# ADR 0019: Markdown-First Dual Surface Output and Dead Metadata Compaction

## Status

Accepted for [Milestone 054](../milestones/054-markdown-first-dual-surface-and-agent-ergonomics.md).
The owner admission authorizes the Markdown default prescribed by that milestone.
This decision amends Decision 2 of [ADR 0001](0001-dual-surface-parity.md).
[ADR 0011](0011-bounded-mcp-response-budgets.md) response bounds and
[ADR 0016](0016-lean-stage-tailored-task-context-bundles.md) task guidance protections remain binding.

## Context

Agents and terminal users need readable code intelligence, task guidance, and
diagnostics. A shared presentation owner prevents the two protocol adapters from
diverging. Programmatic callers need an explicit structured format, stable
semantic results, and working continuation tokens.

## Decision

### Shared output owner and default

Both surfaces default to structured GitHub-flavored Markdown. MCP requests accept
`format: "markdown" | "json"`; the CLI accepts global
`--format markdown|json`. Omission means Markdown. Invalid values fail before
executing the operation. Format selection is a runtime presentation option;
it adds no persisted configuration field.

Pure formatting and response compaction have one canonical owner under
`src/core/format/`, wired through `src/core/mod.rs`. Reuse or move the existing
pure compaction rules from `src/mcp/metadata.rs`; preserve compatibility
re-exports when moving shared public helpers. Core remains a strict leaf under
[ADR 0018](0018-layer-dependency-boundaries-and-core-leaf-isolation.md).
Adapters supply operation identity, validated semantic data, and resolved workspace
coordinates. They retain transport framing, error status, and byte-budget enforcement.

For an operation available through both surfaces, equivalent requests use the
same semantic projection and formatter. Markdown text is identical for equivalent
data and presentation options; JSON values are equivalent after parsing. Framing,
trailing newline, transport identity, and genuinely changing runtime values are
outside byte equality. CLI-only administration, index maintenance, and milestone
lifecycle commands remain CLI-only. This decision adds no MCP operations.

### Scoped compaction and semantic integrity

Markdown and JSON share operation-aware compaction. Omit inactive presentation
metadata and empty optional graph sections. JSON remains a structured object or
scalar of the same operation; nonempty domain data and meaningful zero/false
values remain observable. Never remove fields recursively because their names
look like metadata.

Preserve every active collection continuation, including nested pages, source
previews, and Git-change fingerprints. Continuation hints carry the original
selection, next offset or source offset, and accepted generation token. Determine
continuation requirements after response-size trimming: a formerly complete page
that loses tail items must retain the token needed to fetch them. Source
continuation alone also requires generation. A completed collection does not
justify removing a token used by a different collection.

Keep full durable evidence identities, candidate/content hashes, gate receipts,
checkpoint user content, SQL row values, and actionable task guidance. Existing
stage tailoring, explicit detail levels, permitted bounding, selector rejection,
SQLite timeout, and response byte ceilings remain binding. Oversized indivisible
items fail with recovery guidance; formatting never silently drops them.

Task context, claim, inspect, and submit retain applicable invariants, dependencies,
write scope, subtask DoD, active rules, evidence templates, exact proof commands,
review findings, candidate bindings, and delivery receipts. Deduplicate identical
presentation only when all distinct governing content remains accessible in the
same response under the existing bounded operation contract.

### Coordinates and diagnostics

Render file, symbol, and document coordinates as absolute `file:///` links with
actual line fragments when known. Resolve links against the selected worktree or
linked workspace. Encode URI-sensitive characters. Directory links have no
invented line number; unavailable coordinates remain explicit text.

Render errors and notices as `[!CAUTION]`, `[!WARNING]`, or `[!NOTE]` callouts
with the existing error code, cause, and supported next step. Preserve MCP
`isError`, CLI exit behavior, fail-closed validation, and JSON diagnostics.
Use actual command names and available arguments. Missing optional configuration
is not an error: Forge supports defaults without `forge-mcp.yaml`.
AST diagnostics use the selected language profile's examples and guidance.

## Output contracts and admission boundaries

The [Markdown contracts](../reference/mcp-markdown-contracts.md) own field-level
presentation, producer/consumer bindings, and verification examples. Milestone 054
owns implementation order and proof obligations.

The admitted overview includes manifest-backed Cargo, Python/Poetry, npm/pnpm/yarn,
and Go component discovery, active milestones, documentation hubs, and a
resolution-health calculation. Existing directory components must not be relabeled
as manifest-backed packages. Producers missing from the baseline belong to the
discovery task; formatting must not invent them.

Existing Git-change impact, verified test roles and call paths, and document
diagnostics remain observable. Positive/negative test intent has no baseline
producer. Its unresolved authority and execution boundary are recorded in the
milestone; this ADR does not authorize inferred intent badges.

## Consequences and non-goals

Agents receive compact readable results while scripts select JSON explicitly.
Public format defaults change and documentation, schemas, and test clients
co-evolve with the switch. Presentation does not change storage schemas, graph
truth, ACDD gate semantics, supported transports, or authorization. No token-saving
percentage or performance benchmark is promised by this decision.
