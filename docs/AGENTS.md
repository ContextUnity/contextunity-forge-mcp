---
title: "Documentation and planning governance"
doc_type: guide
---

# Documentation and planning governance

## Authority and style

Read the [root instructions](../AGENTS.md) and relevant owner instructions.
Verify runtime claims against live Rust source, configuration, and tests.
Write documentation in English; use Ukrainian for illustrative narrative examples.
Describe inputs, effects, results, and failure conditions directly in present tense.
Express instructions as affirmative imperatives (`Use`, `Write`, `Keep`, `Verify`, `Preserve`).
Preserve explicit validation boundaries and security checks.

## Canonical ownership

Keep current topology in `architecture/`, interfaces in `reference/`, operations
in `runbooks/`, and test guidance in `testing/`. Keep each contract in one canonical
page and link its consumers. Record accepted decisions in `adr/` under owner
approval; delivery agents escalate decision changes to that process.

Use `roadmap.md` for one or two paragraphs of strategic context. Read admitted
commitments, ordering, and dependencies from `milestones/`. Keep research,
proposals, and source plans in `plans/`; milestone admission creates an admitted commitment.

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
   doc_type: architecture # architecture | adr | api | guide | contract | plan
   ---
   ```
   Forge uses `doc_type` for filtered documentation search (`search_docs`).

2. **Heading chunking**:
   Use clear second-level (`##`) and third-level (`###`) headings. Forge splits
   documents at headings into indexed sections (`DocSection`). Each section forms
   an independent BM25 full-text search unit.

3. **Symbol linkage**:
   Enclose code symbols in backticks matching `[a-zA-Z_][a-zA-Z0-9_.:]*`:
   - Examples: ` `LanguageProfile` `, ` `source_inventory` `, ` `crate::engine::scanner` `, ` `SessionCheckpoint.save` `.
   - Forge automatically extracts backticked symbols and creates bidirectional
     graph edges between documentation sections and code entities.
   - Code inspections (`code_map_inspect`, `code_map_explain`) return linked
     documentation sections alongside signature and relationship evidence.

4. **Architectural invariants**:
   Declare architectural invariants within GitHub alert callouts containing `Invariant:`:
   ```markdown
   > [!IMPORTANT]
    > Invariant: Every reader holds the shared database snapshot lock while reading and rejects a result if the snapshot identity changes.
   ```
   Supported alert tags: `[!IMPORTANT]`, `[!WARNING]`, `[!NOTE]`.
   Forge marks matching sections with `is_invariant: true`, indexing them for
   prioritized display during impact analysis and symbol inspection.

5. **Relative links**:
   Resolve cross-document links using relative repository paths.

## Planning and documentation alignment

Preserve plan content and relationships during transfer. Admit approved
execution commitments into `milestones/` and link plans and milestones both ways.
Plan 1-5 coherent feature-slice tasks initially. Add related discoveries to the
active milestone while its goal, ownership, and invariants apply.

Read the [task reference](reference/tasks.md) for registered task operations,
lifecycle gates, and SQLite storage configuration.

Include stale architecture or runbook pages in the authorized write scope and
align them with verified source. Apply clear accepted rules autonomously;
escalate direct decision conflicts and irreversible database or data-loss choices.
Preserve source-plan requirements until reconciliation proves their destination.
Validate metadata, relative links, scan admission, and documentation retrieval.
Obtain explicit approval for commits and publication under the root instructions.

## Worktree merge and milestone reconciliation

When merging a development worktree or branch into `main` after completing feature slices:
1. **Inspect milestone authority**: Run `contextunity-forge-mcp milestone list` and `contextunity-forge-mcp milestone show <id-or-prefix> --full` to select the governing contract.
2. **Close completed tasks**: Submit each task's `deliver/v1` gate and confirm its generated receipt in the milestone YAML block. Confirm completion with `contextunity-forge-mcp task list --status all`.
3. **Verify milestone delivery**: Run `cargo test --all-targets`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --test commitment_integrity`; record the command and test counts for the milestone receipt.
4. **Archive the milestone**: Run `contextunity-forge-mcp milestone handoff <id-or-prefix> --verification-command <command> --tests-passed <count> --tests-failed 0`. The CLI validates SQLite completion, writes the handoff receipt, updates task references, and moves the document into `docs/milestones/archive/`.
5. **Reconcile before pruning**: Commit the milestone and task documentation with the verified implementation, then remove the completed worktree with `git worktree remove`.
