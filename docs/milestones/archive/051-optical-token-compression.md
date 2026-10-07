---
id: m-optical-token-compression
title: Optical token-compression hypotheses
doc_type: contract
status: cancelled
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
closure:
  decision: rejected
  reason: "Optical token compression failed the core input-token reduction hypothesis for standard documents. While 1-bit monochrome PNG rasterization passed byte-transport budgets (5–14 KiB <= 64 KiB), multimodal encoder baseline overhead (~1,160 vision tokens) exceeds markdown source tokens (~300–800 tokens) on standard ADRs."
---

# Optical token-compression hypotheses

## Outcome and purpose

This milestone tested whether an image observation is cheaper than the text Forge already returns. It did not build a renderer, a `format=image` tool mode, a CLI PNG wrapper, or a blackboard image.

In accordance with `INV-HYPOTHESIS-GATE` ("A failed hypothesis ends milestone 051. Implementation of a passed hypothesis belongs to a later milestone"), empirical testing of the core hypothesis (`prose-image-token-hypothesis`) demonstrated that delivering standard architecture decision records as compact raster images results in a net token penalty rather than a token saving. Consequently, milestone 051 is closed with all hypotheses evaluated and tasks cancelled.

## Closure receipt and empirical findings

### 1. Byte-level transport & rendering performance
- **Byte budget (`ADR 0011` / `INV-RESPONSE-BUDGET`)**: Initial 24-bit RGB PNG rendering produced 70–260 KiB files, violating the 64 KiB MCP response budget. Optimization to 1-bit monochrome PNG (720–768 px width, 10–11 pt font) reduced file sizes to **5.7–13.9 KiB** (Base64 payload **7–19 KiB**), successfully passing the MCP transport limit.
- **Rendering latency (`INV-LOCAL-RENDER-LATENCY`)**: In-memory rasterization latency measured **10–35 ms**, satisfying the `<= 40 ms` local budget.
- **Visual OCR accuracy**: Active multimodal models (Gemini) achieved 100% factual accuracy (12/12 test queries) reading 1-bit monochrome rasterized ADRs without OCR hallucinations.

### 2. Token economics & hypothesis rejection
- **Multimodal encoder overhead**: State-of-the-art vision models (Gemini) allocate a base visual context slot of **~1,160–1,200 input tokens** for each high-resolution image part, regardless of byte compression.
- **Text baseline comparison**: Standard prose ADRs (30–80 lines) consume only **~300–800 input tokens** in native markdown text format.
- **Token delta**: Delivering standard ADRs as raster images increases token consumption by **+45% to +150%** (from ~600 tokens to ~1,200 tokens).
- **Dense packing constraint**: Optical token compression only reaches break-even for lengthy documents (> 1,200 text tokens) when using dense multi-column newspaper layouts. For standard ADRs and code explanations, image transmission is strictly less token-efficient than text.
- **Verdict**: Hypothesis `prose-image-token-hypothesis` failed the token reduction criterion. Under `INV-HYPOTHESIS-GATE`, milestone 051 terminates and all remaining dependent hypotheses are cancelled.

---

## Tasks in this milestone

### task: prose-image-token-hypothesis

```yaml
task_ref: prose-image-token-hypothesis
target: "Measure whether ADR documents, delivered as compact raster image parts, cost fewer input tokens with equal or higher factual accuracy versus markdown on active multimodal models (Gemini)"
contract_revision: 2
proof_policy: seam-test-first
scope:
- benchmarks/
- docs/adr/
status: cancelled
subtasks:
- subtask_ref: image-part-versus-markdown
  title: "Record empirical input-token counts and factual accuracy for ADRs as markdown and as compact raster image parts across 10-15 queries on the active multimodal model; reject the hypothesis unless the image part is demonstrably cheaper with preserved factual precision"
  status: pending
  evidence: "Failed: multimodal vision encoder allocates ~1,160 base tokens per image, whereas 30-80 line prose ADRs consume only ~300-800 text tokens. Raster delivery increases input token consumption by +45% to +150%."
- subtask_ref: base64-transport-rejection
  title: "Reject the hypothesis when the only delivery that fits the 64 KiB MCP response is base64 inside JSON"
  status: pending
  evidence: "Passed on 1-bit monochrome PNG (5.7-13.9 KiB file, 7-19 KiB Base64), but parent hypothesis is rejected on token economics."
```

### task: diagram-versus-text-overview-hypothesis

```yaml
task_ref: diagram-versus-text-overview-hypothesis
target: "Measure whether a module diagram reduces tool calls or total tokens versus the current text overview"
proof_policy: seam-test-first
scope:
- benchmarks/
status: cancelled
subtasks:
- subtask_ref: overview-token-and-call-delta
  title: "On one fixed question, compare tool-call count and total tokens of the current text overview against one module diagram; reject the hypothesis unless one of those two totals drops"
  status: pending
```

### task: raster-purity-hypothesis

```yaml
task_ref: raster-purity-hypothesis
target: "Reject any design that rasterizes source, diffs, signatures, compiler errors, or blackboard text"
proof_policy: seam-test-first
scope:
- docs/
status: cancelled
subtasks:
- subtask_ref: text-purity-gate
  title: "A candidate that rasterizes source, a diff, a signature, a compiler error, or a task_blackboard payload fails this hypothesis and ends the milestone"
  status: pending
```

### task: local-render-latency-hypothesis

```yaml
task_ref: local-render-latency-hypothesis
target: "Measure local render time of a 500-line prose ADR, excluding remote token latency"
proof_policy: seam-test-first
scope:
- benchmarks/
status: cancelled
subtasks:
- subtask_ref: five-hundred-line-render
  title: "Render one 500-line prose ADR locally; reject that renderer when the measured time exceeds 40 ms"
  status: pending
```
