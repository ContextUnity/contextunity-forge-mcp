---
title: "Documentation and planning governance"
doc_type: guide
---

# Documentation and planning governance

## Authority and language

Read the [root instructions](../AGENTS.md) and relevant owner instructions.
Verify runtime claims against live Rust source, configuration, and tests.
Write documentation in English; use Ukrainian for illustrative narrative examples.
Describe inputs, effects, results, and failure conditions directly.
Preserve explicit validation failures and security boundaries.

## Canonical ownership

Keep current topology in architecture/, interfaces in reference/, operations
in runbooks/, and test guidance in testing/. Keep each contract in one canonical
page and link its consumers. Record accepted decisions in adr/ under owner
approval; ordinary delivery agents escalate decision changes to that process.

Use roadmap.md for one or two paragraphs of strategic context. Read admitted
commitments, ordering, and dependencies from milestones/. Keep research,
proposals, and source plans in plans/; admission creates a linked milestone.

## Indexing and source linkage

Start every Markdown page with valid YAML containing title and doc_type.
Use architecture, adr, api, guide, or contract as doc_type.
Use focused second/third-level headings and exact code identifiers in backticks
without call parentheses. Resolve relative links from the owning document.
Declare architectural rules in supported alert callouts containing Invariant:.

> [!IMPORTANT]
> Invariant: Current-system documentation states behavior verified against live
> source. Plans and target contracts identify their admission state explicitly.

Keep default indexed roots explicit in forge-mcp.yaml. Admit current contracts
and milestones, including milestones/archive/. Keep plans/ and the general
archive/ outside default indexed roots. Verify scan admission after changes.

## Planning and documentation alignment

Preserve plan content and relationships during transfer. Admit only approved
execution commitments into milestones/ and link plans and milestones both ways.
Plan 1-5 coherent feature-slice tasks initially. Add related discoveries to the
same active milestone while its goal, ownership, and invariants apply.

The [task lifecycle milestone](milestones/010-repository-task-lifecycle.md)
owns the target workflow and its activation evidence. Use registered tools
from the [current MCP reference](reference/mcp-tools.md) for current work.

Include stale architecture/runbook pages in the authorized write scope and
align them with verified source. Apply clear accepted rules autonomously;
escalate direct decision conflicts and irreversible database/data-loss choices.
Preserve source-plan requirements until reconciliation proves their destination.
Validate metadata, relative links, scan admission, and documentation retrieval.
Obtain explicit approval for commits and publication under the root instructions.
