# Recommended Agent Skills for ContextUnity Forge

This directory contains the primary and recommended companion skills for AI coding agents operating within Forge-indexed repositories.

---

## Skills Index

| Skill | Description | Primary Role / Phase |
|---|---|---|
| [`contextunity-forge`](contextunity-forge/SKILL.md) | **Primary Forge Skill**: Guides agents in navigating the 20 MCP tools, running the canonical code discovery workflow, and managing ACDD task lifecycles. | All Agent Sessions |
| [`test-contract-seam`](test-contract-seam/SKILL.md) | Guides the authoring of red seam tests (TDD) that prove behavioral contracts through observable production interfaces rather than private mocks. | `contract` & `build` Gates |
| [`test-suite-refactor`](test-suite-refactor/SKILL.md) | Governs splitting monolithic test suites, improving typing, and eliminating duplicate or brittle tests without coverage regression. | Refactoring & Hardening |
| [`agent-instructions`](agent-instructions/SKILL.md) | Standards for authoring and revising `AGENTS.md`, `SKILL.md`, and project documentation following affirmative, imperative guidelines. | Governance & Docs |
| [`close-review-findings`](close-review-findings/SKILL.md) | Methodical framework for analyzing review feedback, attributing origins, proving root causes, and executing verified invariant repairs. | `review` & Defect Remediation |
| `review-authority` (external prerequisite; see setup below) | Governs which findings may change scope, contracts, or implementation. Required by `close-review-findings`. | Review and repair authority |
| [`lean-ctx`](lean-ctx/SKILL.md) | Tools for token-efficient source inspection, compressed shell output, and directory navigation to minimize LLM context window consumption. | Code Reading & Execution |

---

## Installation & Setup

`close-review-findings` requires the external `review-authority` skill. Install
it from the [shared agent skills repository](https://github.com/alpiua/dot-agents)
before using the bundle:

```bash
git clone --depth 1 https://github.com/alpiua/dot-agents.git /tmp/forge-shared-agent-skills
mkdir -p ~/.agents/skills
cp -r /tmp/forge-shared-agent-skills/skills/review-authority ~/.agents/skills/
```

`review-authority` governs finding admission and repair authority and is an
external prerequisite, rather than a bundled companion. Verify that
`~/.agents/skills/review-authority/SKILL.md` is present before invoking
`close-review-findings`.

To make these skills globally available to your agent (e.g., in Claude Desktop, Cursor, or CLI agents):

```bash
# Example: Copy skills to your global agent skills directory
mkdir -p ~/.agents/skills
cp -r skills/* ~/.agents/skills/
```

In your repository's `AGENTS.md`, reference the required skills as described in [skills/contextunity-forge/references/agents.md.example](contextunity-forge/references/agents.md.example).
