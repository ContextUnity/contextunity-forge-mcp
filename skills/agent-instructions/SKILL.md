---
name: agent-instructions
description: >
  Create or revise AGENTS.md, agent workflows, SKILL.md instructions, and
  documentation. Use when adding, shortening, routing, reviewing, or removing
  agent guidance in a repository, and for any docs, ADR, README, or comment
  change.
---

# Agent Instructions

## Boundaries

```text
~/.agents/skills/<name>/SKILL.md                reusable base procedure
<repository>/AGENTS.md                          router, commands, and guards
<repository>/.agents/skills/<name>/ADDON.md     optional repository add-on
<repository>/**/AGENTS.md                       subtree-specific commands and invariants
<repository>/.agents/{skills,workflows}         repository-owned procedures and adapters
<repository>/docs/**                            contracts, architecture, and operational detail
```

## Routing Principle

Every shared skill is a base contract, not a workspace-specific workflow. The
repository that invokes it supplies its own add-on when local commands, tools,
evidence, or boundaries are required:

```text
user request
  -> repository AGENTS.md selects the shared skill
  -> shared SKILL.md establishes the reusable procedure
  -> optional .agents/skills/<skill>/ADDON.md supplies repository behavior
  -> nearest subtree AGENTS.md supplies narrower ownership rules
```

The add-on may add repository-specific routes, commands, tool adapters, evidence
locations, and stricter gates. It must not weaken the shared skill's safety,
authorization, or evidence requirements. Do not depend on an enclosing workspace
`AGENTS.md`: agents start from an individual Git repository.

For Forge-managed tasks, route entry through `contextunity-forge` and the active
task's workflow guidance. Forge owns gate and evidence semantics; repository
instructions own commands, test lanes, and permissions. A local ACDD reference
links the engine contract and records owner-specific constraints. For other
task engines, route to their admitted contract. Load specialized skills for
the current action when the owner requires them.

## Write

1. Read the target repository root and nearest parent `AGENTS.md` files.
2. Put cross-repository topology only in the root `AGENTS.md`; put commands and
   non-negotiable rules with the repository or subtree that owns them.
3. Route reusable actions by global skill name, for example
   `close-review-findings` or `commit-workflow`. When the repository needs local
   behavior, route the skill to `.agents/skills/<skill>/ADDON.md` after loading
   the global base skill.
4. Give each rule one owner. Do not copy a workflow or a detailed contract into
   multiple `AGENTS.md` files.
5. Use imperative, copyable language: `Read`, `Run`, `Load`, `Use`, `Do not`.
6. Keep Markdown links repository-relative and valid from the file that owns
   them. Use a skill name rather than a filesystem link for `~/.agents` skills.
7. Keep one add-on per shared skill per repository. Do not create a local
   `SKILL.md` with the same name, because it can shadow the global base skill.

## Documentation Style

Write what the system is now and what to do with it. Present tense.

- Delete a file whose only subject is a removed system. Do not keep it as
  history, archive, or a status banner.
- ADRs record the accepted end state only: decision, current shape,
  consequences, non-goals. Replace the decision when it changes. Do not narrate
  prior stacks or intermediate designs.
- `AGENTS.md`, skills, and comments: one short imperative per rule. Owner path
  and verb (`Read`, `Write`, `Load`, `Use`, `Keep`).
- `docs/` holds current architecture and runtime contracts. Planner holds task
  state. Skills hold procedure. Generated context dumps are not SSoT.
- Do not write migration history. Do not justify a platform rule with one
  tenant vertical. Examples stay generic unless the name is a runtime type.

## Keep It Short

- Repository `AGENTS.md`: routes, commands, guards, and sibling-repository
  boundaries.
- Subtree `AGENTS.md`: only local rules that differ from the repository root.
- Workflow: ordered task steps and command gates.
- Skill: reusable tool selection, procedure, and validation.
- Docs: detailed contracts, schemas, rationale, and examples.

Do not write session history, migration history, product-specific detail in a
shared skill, unregistered tool names, or duplicate procedures.

## Validate

1. Resolve every repository-relative Markdown link changed by the edit.
2. Run the repository's instruction or layout checks when available.
3. Every sentence describes current runtime or an action; ADR Context has no
   former stack and no niche origin story.
4. Run `git diff --check` from every changed Git repository.
