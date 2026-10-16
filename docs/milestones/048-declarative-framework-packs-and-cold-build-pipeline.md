---
id: m-declarative-framework-packs-and-cold-build-pipeline
title: Declarative framework packs, cold build pipeline optimization, and workspace alignment
doc_type: contract
status: active
depends_on:
- m-indexer-allocation-delta-and-dispatch-precision
owners:
- src/db/writer.rs
- src/engine/scanner/
- src/engine/languages/dependency_registry.rs
- src/engine/languages/manifests/
- src/engine/languages/vue.rs
- src/engine/ast/routes.rs
- src/engine/linker/
- skills/contextunity-forge/
- docs/
invariants:
- 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository copy (commerce-release-update) must not regress; eliminating redundant file I/O and secondary filesystem traversals must reduce overall cold build latency.'
- 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions by default, permitting granular rule enrichment without duplicating standard library tables.'
- 'INV-FRAMEWORK-DISCOVERY: Framework manifest rules activate when explicitly placed in .forge/frameworks/ or declared in package manifests, enabling template-driven libraries like Alpine.js, HTMX, and GridViewSpec to resolve without mandatory package.json entry.'
- 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic and pass cargo test --all-targets.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
started_at: 2026-10-16T00:37:50+00:00
---

# Declarative framework packs, cold build pipeline optimization, and workspace alignment

## Outcome and purpose

Deliver an end-to-end resolution of the architectural, documentation, and indexing gaps identified in the comprehensive ContextUnity Forge audit:
1. Align documentation, ADR records (0001, 0015, 0016), task profiles, and agent skills with current runtime reality; provide production-ready `.forge/frameworks/*.toml.example` reference manifests. (Completed in Task 1).
2. Optimize the cold build pipeline by eliminating double file reads and double SHA-256 hashing between scanner and writer, removing the redundant secondary directory walk in `DependencyRegistry` while preserving root-level manifest discovery, and pooling Tree-sitter parsers across all supported languages.
3. Upgrade framework manifest semantics to support declarative extension (deterministic merge with bundled rules where project rules take precedence), automatic activation when placed in `.forge/frameworks/`, and bundled `gridviewspec.toml`.
4. Align configuration, gitignore guards, and linked workspaces in the control repository `commerce-release-update`, `grid-view-spec`, and `traverse`.

---

## Tasks in this milestone

### task: documentation-skills-and-reference-alignment

```yaml
task_ref: documentation-skills-and-reference-alignment
target: Align documentation, task profiles, reference examples, and agent skills with verified runtime contracts and schemas
proof_policy: direct-proof
contract_revision: 1
scope:
- docs/reference/profiles.md
- docs/reference/configuration.md
- docs/reference/tasks.md
- docs/reference/framework-manifests.md
- docs/architecture/indexing.md
- README.md
- TESTS.md
- docs/testing/README.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/
- .forge/
status: completed
subtasks:
- subtask_ref: fix-docs-and-profiles-schema
  title: Correct GateProfile and ACDD profile schema examples in docs/reference/profiles.md and clarify TASK_GUIDANCE_MISSING triggers in configuration.md and tasks.md
  status: completed
  evidence: Aligned GateProfile schema in docs/reference/profiles.md, clarified TASK_GUIDANCE_MISSING in configuration.md, tasks.md, and README.md, fixed query sql and build stage references.
- subtask_ref: align-shared-skills-and-add-framework-examples
  title: Update skills/contextunity-forge/SKILL.md, fix forge-mcp.yaml.example and acdd_profile.yaml.example, add django, alpinejs, and gridviewspec reference examples, and sync installed skills
  status: completed
  evidence: Updated SKILL.md, fixed forge-mcp.yaml.example and acdd_profile.yaml.example, created django, alpinejs, and gridviewspec reference examples in references/frameworks/.
- subtask_ref: clean-obsolete-root-profile
  title: Remove obsolete .forge/acdd_profile.yaml and align README.md and TESTS.md
  status: completed
  evidence: Removed obsolete .forge/acdd_profile.yaml from workspace root.
receipt:
  contract_revision: 1
  passed_at: 2026-10-16T00:45:54.015346067+00:00
  evidence:
    command_proof:
      command: cargo test --test core
      exit_code: 0
      tests_passed: 31
      tests_failed: 0
      log: Docs, profiles, and skills aligned with verified runtime schemas
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All modified and added files stay strictly within the admitted task write perimeter.
        claims:
          applicable: true
          evidence: GateProfile schemas deserialize cleanly, TASK_GUIDANCE_MISSING triggers match workspaces.rs, and reference examples provide valid django, alpinejs, and gridviewspec manifests.
        concurrency:
          applicable: true
          evidence: Markdown and YAML edits do not touch runtime concurrency guards or locks.
        project_isolation:
          applicable: true
          evidence: Changes are strictly confined to forge-mcp.
        administration:
          applicable: true
          evidence: Removed obsolete root profile .forge/acdd_profile.yaml; active profile intact.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository copy (commerce-release-update) must not regress; eliminating redundant file I/O and secondary filesystem traversals must reduce overall cold build latency.'
    - 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions by default, permitting granular rule enrichment without duplicating standard library tables.'
    - 'INV-FRAMEWORK-DISCOVERY: Framework manifest rules activate when explicitly placed in .forge/frameworks/ or declared in package manifests, enabling template-driven libraries like Alpine.js, HTMX, and GridViewSpec to resolve without mandatory package.json entry.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic and pass cargo test --all-targets.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `docs/reference/profiles.md` YAML examples validate against strict `GateProfile` deserialization.
2. `docs/reference/configuration.md` accurately describes `TASK_GUIDANCE_MISSING` conditions matching `workspaces.rs`.
3. `skills/contextunity-forge/references/` contains valid `forge-mcp.yaml.example`, `acdd_profile.yaml.example`, and `.forge/frameworks/*.toml.example` files.
4. Installed global skill `~/.agents/skills/contextunity-forge/` matches the updated repository skill.
5. All tests and docs checks pass.

---

### task: cold-build-pipeline-and-parser-optimization

```yaml
task_ref: cold-build-pipeline-and-parser-optimization
target: "Eliminate cold build I/O bottlenecks and optimize parser pooling without regressing indexing fidelity"
proof_policy: seam-test-first
contract_revision: 5
scope:
  - src/engine/scanner/mod.rs
  - src/db/writer.rs
  - src/engine/languages/dependency_registry.rs
  - src/engine/languages/mod.rs
  - src/engine/ast/mod.rs
  - src/engine/languages/vue.rs
  - docs/reference/cli.md
  - docs/architecture/concurrency.md
  - benchmarks/
  - tests/core/storage_batches.rs
  - tests/languages/typescript/frameworks.rs
status: ready
subtasks:
  - subtask_ref: eliminate-double-file-read-and-digest
    title: "Read and hash source files exactly once during cold build extraction within configurable RAM budget (limits.memory_budget_mb and FORGE_MEMORY_BUDGET_MB); benchmark on commerce-release-update across 50, 200, 500, 1000 MB to determine optimal throughput-to-memory operating point"
    status: pending
  - subtask_ref: eliminate-dependency-registry-second-walk
    title: "Collect dependency manifest paths during scanner traversal while preserving discovery for root manifests outside source roots via direct existence checks, eliminating redundant WalkBuilder filesystem walks"
    status: pending
  - subtask_ref: expand-parser-pool-and-single-pass-vue
    title: "Extend thread-local PARSER_POOL to all languages and optimize Vue script setup traversal"
    status: pending
  - subtask_ref: align-cli-and-concurrency-docs
    title: "Align docs/reference/cli.md on open_or_rebuild auto-index behavior and update docs/architecture/concurrency.md to 20 MCP tools and modular task store paths"
    status: pending
```

#### Acceptance

1. Cold build does not read source files twice from disk during indexing.
2. `DependencyRegistry` collection eliminates the redundant `WalkBuilder` filesystem walk by consuming scanner manifest paths and performing direct root-level manifest probes (`root.join("package.json")`) when `roots` excludes project root.
3. Configurable RAM budget is supported via `limits.memory_budget_mb` in `forge-mcp.yaml` and `FORGE_MEMORY_BUDGET_MB` environment variable (falling back to adaptive system RAM calculation); cold build benchmarks on `commerce-release-update` across 50, 200, 500, and 1000 MB record duration vs peak RSS and document the optimal operating point.
4. Parser pooling is available for all active language profiles.
5. `docs/reference/cli.md` and `docs/architecture/concurrency.md` reflect verified runtime reality.
6. Cold build throughput on `commerce-release-update` is measurably improved; `cargo test --all-targets` passes with zero regressions.

---

### task: declarative-framework-packs-and-cross-repo-alignment

```yaml
task_ref: declarative-framework-packs-and-cross-repo-alignment
target: "Implement framework manifest extension mode, auto-activation, GridViewSpec support, and workspace alignment"
proof_policy: seam-test-first
contract_revision: 3
scope:
  - src/engine/languages/manifests/framework.rs
  - src/engine/languages/dependency_registry.rs
  - src/engine/languages/manifests/
  - docs/reference/framework-manifests.md
  - docs/architecture/modularity.md
  - docs/adr/0017-serde-framework-manifest-formats.md
  - tests/languages/manifests.rs
status: ready
subtasks:
  - subtask_ref: framework-manifest-merge-and-workspace-activation
    title: "Implement extend: true merge mode in FrameworkManifest with project rule precedence and allow activation from workspace .forge/frameworks/ directory"
    status: pending
  - subtask_ref: add-gridviewspec-and-rich-framework-rules
    title: "Add bundled gridviewspec.toml manifest with receiver/route schema validation and auto-activation"
    status: pending
  - subtask_ref: align-connected-workspaces
    title: "Align forge-mcp.yaml and .forge/ in commerce-release-update, grid-view-spec, and traverse"
    status: pending
```

#### Acceptance

1. Project `.forge/frameworks/<id>.toml` (and `.yaml`) files extend bundled definitions with project rules taking precedence over bundled defaults, and support explicit `replace: true`.
2. Placing a manifest in `.forge/frameworks/` activates framework rules even when undeclared in `package.json` / `pyproject.toml`, reconciled with ADR 0017 and `docs/architecture/modularity.md`.
3. `gridviewspec.toml` is bundled and validates under framework manifest schema; behavioral execution of its receivers and routes is accepted in milestone 021 (task `rust-to-toml-rule-externalization`) once manifest routes/receivers are wired to extraction and linking.
4. Connected workspace adapter validation in Forge MCP verifies that configuration in `commerce-release-update`, `grid-view-spec`, and `traverse` parse cleanly, and documents prerequisites for removing obsolete `pim` references in external workspace adapters.
