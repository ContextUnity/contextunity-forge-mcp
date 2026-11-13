# Contract planning — ContextUnity Forge MCP add-on

This add-on supplies repository-specific planning authority, invariants, and inspection
tools for ContextUnity Forge MCP.

---

## 1. Architectural Authority and Reading List

Before authoring, auditing, or amending any milestone contract or task in this repository:

1. **Architecture Overview:** Read [`docs/architecture/README.md`](../../docs/architecture/README.md) and [`docs/architecture/performance.md`](../../docs/architecture/performance.md).
2. **Architectural Decisions:** Consult the relevant ADRs in [`docs/adr/`](../../docs/adr/):
   - `ADR 0018`: Leaf isolation for core modules.
   - `ADR 0021`: Hierarchy between workflows, plans, and milestones.
   - `ADR 0024`: Unified clear, lifecycle segregation, and fail-closed artifact ownership.
3. **Subsystem Routing:** Read [`src/AGENTS.md`](../../src/AGENTS.md) and [`docs/AGENTS.md`](../../docs/AGENTS.md) for module decomposition rules.

---

## 2. Mandatory Architectural Invariants

Every contract must be evaluated against these repository invariants:

1. **`INV-CORE-LEAF` (Subsystem Purity):**
   - `crates/forge-core` is a leaf crate. It must never depend on `crates/forge-index`, `crates/forge-tasks`, CLI, or engine subsystems.
   - Core path helpers must remain fast, standalone primitives without circular dependencies.
2. **`INV-MINIMAL-INTERVENTION` (Binary Leanliness):**
   - Keep the compiled Rust binary focused on indexing, AST search, code maps, and task orchestration.
   - Avoid embedding static markdown documents, templates, or onboarding wizards into the compiled binary via `build.rs` codegen or `include_bytes!`. Distribute companion skills and scripts as clean, external assets.
3. **`INV-ZERO-PERFORMANCE-REGRESSION` (I/O Efficiency):**
   - Path resolution (`checked_child`, `canonical_root`) must remain single-pass checks. Never perform multi-entry directory scans (`read_dir` loops) on path components.
4. **`INV-SCOPE-INTEGRITY` (Protected Infrastructure):**
   - Core configuration files (`Cargo.toml`, root `build.rs`, `.forge/acdd/profile.yaml`) are protected infrastructure. Contracts must not modify them without explicit milestone-level authorization.

---

## 3. Code Discovery and Inspection Seams

Use Forge MCP tools rather than unstructured grep/find during contract planning:

- **Symbol Inspection:** `code_map_inspect(selector: "...")` for full caller/callee trees and covering tests.
- **Impact Analysis:** `code_map_impact(selector: "...", depth: 1)` to trace consumer boundaries before assigning task write scopes.
- **Structural Syntax Search:** `ast_grep_search(pattern: "...", language: "...")` for pattern matching.
- **Targeted Source Reading:** `forge_read(path: "...", selector: "...")` for digest-verified source context.

---

## 4. Integration with ACDD `blueprint_synthesis` Gate and Authoring Workflows

When executing the root `blueprint_synthesis` container (`synthesize` $\to$ `review`) in the active ACDD profile or running standalone authoring workflows (`author-milestone.workflow.yaml`, `author-plan.workflow.yaml`, `author-workflow.workflow.yaml`):

1. Apply the **Architectural Viability Audit** from `SKILL.md` to ensure the milestone, plan, or workflow design is minimal, clean, and robust.
2. Post the synthesized architectural blueprint, data flow, and invariants to `task_blackboard` (topic: `decisions`) before gate submission.
3. Submit the structured `proof` payload (`proof: blueprint` from `proofs:` or inline `proof.scheme`) including per-task `tasks[<task_ref>]` entries (`implementation_steps`, `symbol_signatures`, `test_seam`, `incoming_seams`, `outgoing_seams`, `negative_boundaries`), which the engine fans out into `gate_transitions` (`subject = <full_task_id>`) for automatic per-task context injection on `task claim`.
