---
id: m-optical-token-compression
title: Optical token-compression hypotheses
doc_type: contract
status: planned
depends_on:
- m-language-semantics-and-resolution-coverage:completed
- m-mcp-output-compaction-and-agent-ergonomics:completed
owners:
- benchmarks/
- docs/
invariants:
- 'INV-HYPOTHESIS-GATE: A failed hypothesis ends milestone 051. Implementation of a passed hypothesis belongs to a later milestone.'
- 'INV-CODE-TEXT-PURITY: Source, diffs, signatures, compiler errors, and blackboard payloads stay text.'
- 'INV-TEXT-BASELINE: The comparison baseline is the current text tool output from milestone 050.'
- 'INV-RESPONSE-BUDGET: An image that only fits as base64 inside the 64 KiB MCP response fails the transport hypothesis.'
related_plans: []
---

# Optical token-compression hypotheses

## Outcome and purpose

This milestone tests whether an image observation is cheaper than the text Forge already returns. It does not build a renderer, a `format=image` tool mode, a CLI PNG wrapper, or a blackboard image. Work starts after milestone 020 is completed. Every hypothesis is `blocked` until then.

The baseline is the current text result of `get_doc`, `search_docs`, and `code_map_overview`. Compare an image delivered as a native image part with the same markdown. A design that only embeds base64 in the JSON response is a failed transport, because ADR 0011 caps that response at 64 KiB. Remote token latency is not part of the local 40 ms render measurement.

If a hypothesis fails, milestone 051 stops. A later milestone may implement only a hypothesis that passed.

## Tasks in this milestone

### task: prose-image-token-hypothesis

```yaml
task_ref: prose-image-token-hypothesis
target: "Measure whether one ADR, delivered as an image part, costs fewer input tokens than its markdown on at least two of Claude, GPT, and Gemini"
proof_policy: seam-test-first
scope:
- benchmarks/
- docs/adr/
status: blocked
subtasks:
- subtask_ref: image-part-versus-markdown
  title: "Record input-token counts for one ADR as markdown and as a native image part on at least two of Claude, GPT, and Gemini; reject the hypothesis unless the image part is cheaper on both"
  status: blocked
- subtask_ref: base64-transport-rejection
  title: "Reject the hypothesis when the only delivery that fits the 64 KiB MCP response is base64 inside JSON"
  status: blocked
```

### task: diagram-versus-text-overview-hypothesis

```yaml
task_ref: diagram-versus-text-overview-hypothesis
target: "Measure whether a module diagram reduces tool calls or total tokens versus the current text overview"
proof_policy: seam-test-first
scope:
- benchmarks/
status: blocked
subtasks:
- subtask_ref: overview-token-and-call-delta
  title: "On one fixed question, compare tool-call count and total tokens of the current text overview against one module diagram; reject the hypothesis unless one of those two totals drops"
  status: blocked
```

### task: raster-purity-hypothesis

```yaml
task_ref: raster-purity-hypothesis
target: "Reject any design that rasterizes source, diffs, signatures, compiler errors, or blackboard text"
proof_policy: seam-test-first
scope:
- docs/
status: blocked
subtasks:
- subtask_ref: text-purity-gate
  title: "A candidate that rasterizes source, a diff, a signature, a compiler error, or a task_blackboard payload fails this hypothesis and ends the milestone"
  status: blocked
```

### task: local-render-latency-hypothesis

```yaml
task_ref: local-render-latency-hypothesis
target: "Measure local render time of a 500-line prose ADR, excluding remote token latency"
proof_policy: seam-test-first
scope:
- benchmarks/
status: blocked
subtasks:
- subtask_ref: five-hundred-line-render
  title: "Render one 500-line prose ADR locally; reject that renderer when the measured time exceeds 40 ms"
  status: blocked
```
