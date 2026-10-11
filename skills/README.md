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
| [`lean-ctx`](lean-ctx/SKILL.md) | Tools for token-efficient source inspection, compressed shell output, and directory navigation to minimize LLM context window consumption. | Code Reading & Execution |

---

## Installation & Setup

To make these skills globally available to your agent (e.g., in Claude Desktop, Cursor, or CLI agents):

```bash
# Example: Copy skills to your global agent skills directory
mkdir -p ~/.agents/skills
cp -r skills/* ~/.agents/skills/
```

In your repository's `AGENTS.md`, reference the required skills as described in [skills/contextunity-forge/references/agents.md.example](contextunity-forge/references/agents.md.example).
