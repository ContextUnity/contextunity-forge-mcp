# Hierarchical ACDD Profile Catalog & Domain Examples

This catalog provides production-ready, domain-tailored ACDD v2 profile examples implementing the **Universal Hierarchical Gate Architecture** ([ADR 0020](../../../../docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md)).

---

## 1. Catalog Profiles

| Profile / Manifest File | Target Domain & Tech Stack | Key Architectural Features |
| :--- | :--- | :--- |
| **[`fullstack-commerce-django-playwright.yaml`](fullstack-commerce-django-playwright.yaml)** | Python / Django (ORM, HTMX) + Playwright MCP e2e browser tests | Multi-tier validation: fast scoped backend checks, automated migration dry-run, browser seam verification, and N+1 query contour audits. |
| **[`test-suite-refactor.yaml`](test-suite-refactor.yaml)** | Typed test suite refactoring (aligned with `test-suite-refactor` skill) | Collect-only inventory freezing, duplicate subsumption, strict typing/lint verification, and zero-coverage-loss audit. |
| **[`documentation-graph-refactor.yaml`](documentation-graph-refactor.yaml)** | Documentation graph refactor & ADR reconciliation (aligned with `agent-instructions` skill) | Drift scanning against live signatures, strictly bounded docs/ edits with zero code tampering guard, clickable link validation, and cognitive clarity review. |
| **[`legacy-plans-to-milestones-migration.yaml`](legacy-plans-to-milestones-migration.yaml)** | Legacy plans & tasks to milestone DAG migration (aligned with `planner` skill) | Legacy markdown plan decomposition, topological DAG cycle detection, formal frontmatter/invariant generation, and zero code tampering enforcement. |
| **[`infrastructure-deploy-database.yaml`](infrastructure-deploy-database.yaml)** | DevOps, Terraform, K8s, DB migrations & live rolling release | Preflight dry-run gates, state snapshot checkpoints, forward migration execution, synthetic smoke checks, and automated rollback routing. |
| **[`multirepo-umbrella-cross-service.yaml`](multirepo-umbrella-cross-service.yaml)** | Multi-repo umbrella coordination (backend, frontend, infra) | Upfront cross-service contract synthesis, parallel worktree task delivery, umbrella integration gate, and atomic synchronized handoff. |
| **[`seam-logger-and-event-telemetry.yaml`](seam-logger-and-event-telemetry.yaml)** | Seam telemetry logger & external observability bus dispatch | Heterogeneous task coordination (Code + Doc + Observer), pure topological `when: seam`, and autonomous 0-token Command Gates emitting webhook events and blackboard signals. |
| **[`milestone-manifest-with-skills.md.example`](milestone-manifest-with-skills.md.example)** | Manifest specification demonstrating `required_skills` | Frontmatter declaration of required agent skills, fail-closed `TASK_SKILL_MISSING` validation, and targeted context injection. |

---

## 2. Multi-Repository Lifecycle Management Cycle

When an initiative spans multiple repositories (e.g. `commerce-backend`, `commerce-frontend`, and `infrastructure`):

### The Coordination Architecture

```
                      ┌────────────────────────────────────────┐
                      │ ContextUnity Forge Umbrella Workspace   │
                      │  (Multi-Repo forge-mcp.yaml Index)     │
                      └───────────────────┬────────────────────┘
                                          │
                  ┌───────────────────────┼───────────────────────┐
                  ▼                       ▼                       ▼
      ┌───────────────────────┐ ┌───────────────────┐ ┌────────────────────────┐
      │  commerce-backend     │ │ commerce-frontend │ │   commerce-infra       │
      │  branch: feat/cart-v2 │ │ branch: feat/cart-v2│ │ branch: feat/cart-v2  │
      └───────────┬───────────┘ └─────────┬─────────┘ └───────────┬────────────┘
                  │                       │                       │
                  └───────────────────────┼───────────────────────┘
                                          │
                                          ▼
                      ┌────────────────────────────────────────┐
                      │    Milestone Contract Cross-Matrix     │
                      │       (milestone_contract_matrix)      │
                      │  • Upfront Contract Synthesis Gate     │
                      │  • Cross-repo Seam Mock / Schema Sync  │
                      └────────────────────────────────────────┘
```

### Upfront Preflight Workflow Initialization (`workflow init`)

Before dispatching autonomous agent workers, the coordinator/orchestrator executes preflight initialization:

```bash
contextunity-forge-mcp workflow init docs/milestones/060-hierarchical-acdd-gates-and-milestone-orchestration.md
```

**What `workflow init` executes BEFORE any agent is spawned**:
1. **Host Skill Preflight Check**: Validates that all skills declared in `required_skills:` are physically present in `~/.agents/skills/` or `.agents/skills/`. Fails fast immediately (`WORKFLOW_PREFLIGHT_FAILED`) if a tool is missing, preventing agents from burning tokens in blind sessions.
2. **Multi-Repo Path & Branch Validation**: Reads `repositories:` from manifest frontmatter, confirms all participant repositories exist, and checks that target base branches are clean.
3. **Automated Base Worktree Scaffolding**: If not running in `--in-place` mode, atomically creates milestone base branches (`milestone/<id>`) and base worktrees (`.worktrees/<id>-<slug>`) across all declared repositories in a single operation.
4. **SQLite Workflow Registration**: Admits tasks, registers participant repositories, and populates initial `gate_transitions` root state in `.forge/tasks.sqlite`.

---

### The 4-Stage Multi-Repo Control Cycle:

1. **Preflight Manifest Initialization (`workflow init`)**:
   - Reads `repositories:` and `required_skills:` upfront.
   - Sets up worktrees and base branches automatically across backend, frontend, and infra.

2. **Upfront Cross-Repo Synthesis (`contract_synthesis`)**:
   - The architect runs `contract_synthesis` at the umbrella workspace level.
   - Declares boundary interfaces (e.g. backend publishes OpenAPI schema / HTMX endpoint contract to SQLite `milestone_contract_matrix`).
   - Consumer repositories (frontend) bind their seam tests to this published contract schema before backend implementation starts.

3. **Synchronized Parallel Execution in Scaffolded Worktrees**:
   - Agents work in pre-scaffolded parallel worktrees: `.worktrees/<milestone-prefix>-<repo-name>-<task-slug>`.
   - Each repository's task advances through its own local scoped command gates (`check_task`).
   - SQLite tracks transitions independently under compound breadcrumbs: `{milestone_id}/{repo_id}/{task_id}/{gate_path}`.

4. **Autonomous Cross-Repo Integration Gate (`cross_repo_verify`) & Atomic Merkle Handoff**:
   - Umbrella Command Gate runs integration tests across live services (e.g. Playwright browser tests hitting the live Django backend).
   - Milestone handoff calculates a joint cryptographic receipt binding the Git tree SHAs of all participant repositories before atomic fast-forward landing.

---

## 3. Proposal: Manifest `required_skills` Specification

To eliminate agent hallucination and avoid token-wasting discovery loops across 30+ installed skills, milestone and plan manifests can explicitly declare the skills required for execution:

### Manifest Syntax

```yaml
---
id: m-commerce-checkout-redesign
title: "Commerce Checkout Redesign"
doc_type: contract
status: active
owners:
  - backend/
  - frontend/
# NEW: Explicit Skill Governance
required_skills:
  - contextunity-forge       # Base code map, ast grep, and task operations
  - django-gridviewspec      # Django ORM, HTMX, and GridViewSpec standards
  - playwright               # Browser testing and DOM interaction
  - test-suite-refactor      # Test suite consolidation and coverage preservation
---
```

### Engine Integration Invariants:

1. **Fail-Closed Claim Check (`TASK_SKILL_MISSING`)**:
   When an agent executes `task claim` or `task_manage(action="claim")`:
   Forge checks `~/.agents/skills/` and `.agents/skills/`. If any skill named in `required_skills` is missing or not indexed, claim fails immediately with:
   `TASK_SKILL_MISSING: Skill 'django-gridviewspec' is required by manifest 'm-commerce-checkout-redesign' but not found in active skill registries. Do: install or enable the skill before claiming.`

2. **No Markdown Dumping (Lean Metadata Only)**:
   Forge does **NOT** inject or dump raw skill markdown bodies into the task claim bundle. Invariants, instructions, and `AGENTS.md` already govern agent execution. `workflow_guidance.active_skills` simply returns the lean array of admitted skill names `["contextunity-forge", "django-gridviewspec"]`, acting as an authoritative whitelist without burning context tokens.

3. **Zero Hallucination & Lean Overhead**:
   Agents know exactly which specialized toolsets and standards apply to the task, without guessing or scanning through dozens of unrelated global skills.
