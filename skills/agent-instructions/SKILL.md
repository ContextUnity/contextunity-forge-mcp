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

For Forge-managed tasks, route entry through `contextunity-forge` and the active
task's workflow guidance. Forge owns gate and evidence semantics; repository
instructions own commands, test lanes, and permissions.

## Write

1. Read the target repository root and nearest parent `AGENTS.md` files.
2. Put cross-repository topology only in the root `AGENTS.md`; put commands and
   non-negotiable rules with the repository or subtree that owns them.
3. Route reusable actions by global skill name. When the repository needs local
   behavior, route the skill to `.agents/skills/<skill>/ADDON.md`.
4. Give each rule one owner. Do not copy a workflow or a detailed contract into
   multiple `AGENTS.md` files.
5. Use imperative, copyable language: `Read`, `Run`, `Load`, `Use`, `Do not`.
6. Keep Markdown links repository-relative and valid from the file that owns
   them.

## Documentation Style

Write what the system is now and what to do with it. Present tense.

- Delete a file whose only subject is a removed system.
- ADRs record the accepted end state only: decision, current shape, consequences, non-goals.
- `AGENTS.md`, skills, and comments: one short imperative per rule.
- `docs/` holds current architecture and runtime contracts. Planner holds task
  state. Skills hold procedure.
