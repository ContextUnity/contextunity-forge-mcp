---
title: "Documentation governance"
doc_type: guide
---

# Documentation governance

## Authority and style

Read the [root instructions](../AGENTS.md) and relevant owner instructions.
Verify runtime claims against live Rust source, configuration, and tests.
Write documentation in English; use Ukrainian for illustrative narrative examples.
Describe inputs, effects, results, and failure conditions directly in present tense.
Express instructions as affirmative imperatives (`Use`, `Write`, `Keep`, `Verify`, `Preserve`).
Preserve explicit validation boundaries and security checks.

## Canonical ownership

Follow the documentation layers in the shared `contextunity-forge` skill.
Keep each contract on one page and link its consumers. Record accepted decisions in `adr/` under owner
approval; delivery agents escalate decision changes to that process.

Use `roadmap.md` for one or two paragraphs of strategic context. Read admitted
commitments, ordering, and dependencies from `milestones/`.

## MCP client surface

Keep connection steps in [MCP setup](reference/mcp-setup.md). Keep tool behavior in [MCP tools](reference/mcp-tools.md).

When editing server instructions, tool descriptions, or input-schema text in `src/mcp/tools.rs`, assign each fact to one layer and match it to the handler before writing it. Update [MCP tools](reference/mcp-tools.md) in the same change when the contract changes.

## Forge MCP indexing rules

Forge MCP parses documentation into searchable sections and links them directly
to code symbols in the SQLite code map. Follow these structural standards:

1. **YAML frontmatter**:
   Begin every Markdown file at byte 0 with valid YAML:
   ```yaml
   ---
   title: "Descriptive Document Title"
   doc_type: architecture
   ---
   ```
   Forge uses `doc_type` for filtered documentation search (`search_docs`).

2. **Heading chunking**:
   Use clear second-level (`##`) and third-level (`###`) headings. Forge splits
   documents at headings into indexed sections (`DocSection`). Each section forms
   an independent BM25 full-text search unit.

3. **Symbol linkage**:
   For repository-specific code claims, use the exact indexed declaration name
   and link it to its source file when a path is needed to identify the intended
   declaration. Do not invent a qualified name or present an approximate symbol
   as a verified code claim. For example, this claim names a real declaration
   and explicitly selects its source file:
   [`extract_with_details`](../crates/forge-index/src/docs.rs).
   Ordinary inline code remains useful for commands, configuration keys, and
   other technical text; a code span alone does not certify that its contents
   match an indexed declaration.
   - Forge resolves each code claim against eligible declarations. An explicit
     normalized file target confines eligibility to that file; an unscoped claim
     is limited to the source document's workspace. Document, invariant, and
     component nodes do not verify code claims.
   - Only a unique declaration in the first non-empty match tier creates a
     verified documentation reference. Ambiguous candidates may be shown for
     navigation, but they do not create verified documentation or governance
     edges and do not count as verified coverage. Do not describe candidate
     matches as verified references.
   - See [Documentation queries](reference/mcp-tools.md#documentation-queries)
     for the current eligibility, priority-tier, and candidate-response contract.
   - Code inspections (`code_map_inspect`, `code_map_explain`) return verified
     linked documentation sections alongside signature and relationship
     evidence.

4. **Architectural invariants**:
   A GitHub alert callout containing `Invariant:` marks the section for
   prioritized display during impact analysis and symbol inspection:
   ```markdown
   > [!IMPORTANT]
   > Invariant: Every reader holds the shared database snapshot lock while reading and rejects a result if the snapshot identity changes.
   ```
   Supported alert tags: `[!IMPORTANT]`, `[!WARNING]`, `[!NOTE]`.
   Forge marks such a section with `is_invariant: true`; this section flag alone
   does not create an invariant node or a `governs` edge. To declare a linked
   invariant, put its `INV-*` identifier and an exact code claim within the
   invariant node's line span. Use a source link to confine resolution when needed.
   The declaration must resolve uniquely for Forge to
   create a verified governance edge. This guide declares the resolver's actual
   priority rule:
   - INV-DOC-FIRST-MATCH-TIER: Candidate resolution returns the first non-empty eligible declaration tier. [`document_symbol_candidates`](../crates/forge-index/src/linker/resolution/mod.rs).
   The identifier remains documentation authority; the code link identifies
   the governed declaration. See [documentation queries](reference/mcp-tools.md#documentation-queries)
   for the distinction between invariant discovery and verified governance.

5. **Relative links**:
   Resolve cross-document links using relative repository paths. A link to a
   file without a heading fragment remains useful whole-document navigation;
   add a heading fragment when the link is meant to identify one specific
   indexed section. Check configured roots before treating an existing but
   unadmitted file as a broken filesystem link.
   In historical audits, write file paths as inline code relative to
   the repository root, for example `docs/reference/tasks.md`, instead of links.
   After archiving a document, verify its links and links to its former path.

## Milestone and plan authoring

When creating, reviewing, or materially amending a milestone (`docs/milestones/`) or plan:
- **Mandatory skill**: Load and execute the [`contract-planning`](../skills/contract-planning/SKILL.md) skill and its repository add-on [`ADDON.md`](../skills/contract-planning/ADDON.md).
- **Architectural viability audit**: Evaluate whether the proposed architecture is appropriate and minimal (Occam's razor). Challenge unnecessary code creation or in-binary bloat before committing to task boundaries.
- **ADR and invariant reconciliation**: Reconcile the proposal against `docs/adr/`, `docs/architecture/`, and active repository invariants. Reconcile past architectural trade-offs rather than silently overturning them.
- **Probe weak spots and ambiguities**: Identify ambiguous completion criteria, operational pitfalls (I/O amplification, concurrency bottlenecks, cross-platform traps), and failure modes before implementation starts.
- **Synthesize constructive improvements**: Formulate simpler, durable designs and leverage existing codebase patterns so builder agents have unambiguous, viable targets.

## Milestone documentation alignment

Rely on `workflow_guidance` returned by `task claim` for current gates, operational rules, and review criteria; use the [task reference](reference/tasks.md) for schema definitions and offline operations.

When designing milestone tasks or authoring contracts that modify public interfaces, CLI/MCP tools, lifecycle diagnostics, or architecture, admit the corresponding documentation files (under `docs/`) directly in the task's initial `scope` so code and documentation co-evolve without scope extension friction. Do not defer documentation changes caused by the task's own contract.

Include stale architecture or runbook pages in the authorized write scope and
align them with verified source. Apply clear accepted rules autonomously;
escalate direct decision conflicts and irreversible database or data-loss choices.
Validate metadata, relative links, scan admission, and documentation retrieval.
Follow root `AGENTS.md` ACDD permissions for task/archive commits and their
branch merges. Obtain explicit approval for publication.

## Shared skill and example maintenance

When modifying architecture, workflows, declarative profiles, schemas, CLI commands, or standing Git permissions:
- Update the repository skill [`skills/contextunity-forge/SKILL.md`](../skills/contextunity-forge/SKILL.md) and reference examples under [`skills/contextunity-forge/references/`](../skills/contextunity-forge/references/) (`acdd_profile.yaml.example`, `agents.md.example`, `forge-mcp.yaml.example`, `gitignore.example`) to match active system behavior and schemas.
- Ensure all example configurations parse cleanly and adhere to strict validation invariants (e.g. `deny_unknown_fields`, valid `guidance` keys, and accurate model specs).
- Do not write to external agent directories (`~/.agents/`) directly during task execution; external installation and distribution is handled via installer workflows and dedicated distribution commands.

## Delivery documentation

Follow root [AGENTS.md](../AGENTS.md#worktrees-and-git-permissions) and [TESTS.md](../TESTS.md#milestone-handoff-gate) for verification,
archival, integration, and worktree cleanup. Include changed reference,
architecture, runbook pages, shared skills, and reference examples in the reviewed delivery scope.
Verify that all profile and configuration examples parse and validate cleanly, and that documentation links and frontmatter validate before authorized publication.
