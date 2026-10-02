---
id: m-milestone-lifecycle-and-automation
title: "Milestone CLI automation, structured handoff, and archival"
doc_type: contract
status: active
depends_on:
  - m-repository-task-lifecycle:completed
owners:
  - src/cli/
  - src/engine/
  - src/core/
  - docs/
  - tests/
invariants:
  - "INV-MILESTONE-CLI: Milestone lifecycle operations (list, show, init, handoff) are CLI-only tools without polluting the four-tool MCP contract."
  - "INV-STRUCTURED-HANDOFF: Milestone handoff generates a typed YAML receipt recording duration, git commit, and language-agnostic verification proof."
  - "INV-AUTOMATED-ARCHIVAL: Completing a milestone automatically moves the file to docs/milestones/archive/ and syncs task state."
  - "INV-SEAMLESS-SCAFFOLDING: Milestone init creates the next sequentially numbered milestone from a plan with pre-filled contracts and agent guidance."
  - "INV-STARTED-AT-SEMANTICS: Milestone started_at timestamp remains empty while status is planned; it is stamped with NOW() when activated or upon the first task_claim of the milestone to ensure accurate active development duration."
  - "INV-WORKTREE-RECEIPT-RESOLUTION: Task handoff receipt verification resolves the milestone file within the claimed task worktree directory (task_claims.worktree + milestone_ref), verifying the exact file authored by the claiming agent."
---

# Milestone CLI automation, structured handoff, and archival

## Outcome and purpose

Automate the lifecycle of repository milestones via first-class CLI commands (`init`, `list`, `show`, `handoff`). Eliminate manual error-prone file renaming, manual frontmatter editing, and unstructured free-prose handoff summaries. When all tasks in a milestone reach `completed` status in the task store, `milestone handoff` automatically validates closure, records active development duration and verification proof in a structured `handoff:` YAML block, sets frontmatter `status: completed`, and moves the file to `docs/milestones/archive/`.

## Tasks in this milestone

### task: milestone-init-and-scaffolding

```yaml
task_ref: milestone-init-and-scaffolding
target: "Scaffold next numbered milestone supporting explicit num, slug, plan parsing, and pipe input for chat-driven planning"
proof_policy: seam-test-first
contract_revision: 3
scope:
  - src/cli/mod.rs
  - src/cli/milestone.rs
  - src/engine/milestones.rs
  - tests/core_basics/tasks.rs
status: planned
```

1. **Flexible numbering & prefix rollover**:
   - Support optional `--num <prefix>` flag for explicit numbering (e.g. `--num 011`, `--num 025`, `--num 1050`).
   - If `--num` is omitted, automatically calculate the next logical prefix by scanning existing files in `docs/milestones/` and `docs/milestones/archive/` (e.g. max `050` -> `060`). Format with at least 3 digits (zero-padded) or scale dynamically for 4+ digits without integer overflow or rollover collisions.
2. **Slug and title resolution**:
   - Support explicit `--slug <slug>` to eliminate fragile title-guessing heuristics.
   - Support explicit `--title <title>` and `--depends-on <id>` flags.
3. **Plan file or direct pipe/stdin input**:
   - If `--plan <path>` is provided, extract title, doc_type, purpose, and dependencies from the plan document.
   - If markdown text is piped via stdin (or passed via `--desc <text>` or heredoc `cat << 'EOF' | milestone init ...`), embed the piped description and task notes directly into the generated milestone body between canonical YAML task blocks.
4. **`started_at` lifecycle semantics**:
   - If `--active` flag is passed (or milestone is directly created with `status: active`), initialize `started_at: <ISO-8601-NOW>`.
   - If initialized as `status: planned` (default queue mode), leave `started_at` unpopulated/omitted so queue wait time does not inflate development duration.
5. **Agent guidance output**:
   - Output created file path and actionable guidance on running `contextunity-forge-mcp task sync <path>` and claiming tasks.

---

### task: milestone-list-and-show-inspection

```yaml
task_ref: milestone-list-and-show-inspection
target: "Implement milestone list and show commands displaying milestone metadata, task progress, and YAML structures"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - src/cli/milestone.rs
  - src/engine/milestones.rs
  - src/core/tasks/
  - tests/core_basics/tasks.rs
status: planned
```

1. **`milestone list`**:
   - Scan active milestones in `docs/milestones/` (and archived ones when `--archive` or `--status all/completed` is passed).
   - Display structured table: milestone ID, title, status (`planned`, `active`, `completed`), `started_at` (or `-` if planned/unstarted), and task count with completion ratio (e.g. `6/6 completed`).
   - List constituent task refs and their current state.
2. **`milestone show <id_or_prefix>`**:
   - Resolve milestone by ID (`m-...`) or numeric prefix (`010`, `011`).
   - Output milestone frontmatter, invariants, dependencies, and task YAML blocks.
   - Support `--full` flag to include complete task descriptions, implementation steps, and delivery notes.

---

### task: structured-milestone-handoff-and-archival

```yaml
task_ref: structured-milestone-handoff-and-archival
target: "Implement milestone handoff command validating task completion, computing duration, generating structured handoff YAML, and archiving file"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - src/cli/milestone.rs
  - src/engine/milestones.rs
  - src/core/tasks/gates.rs
  - src/db/tasks_store.rs
  - src/engine/tasks.rs
  - tests/core_basics/tasks.rs
status: planned
```

1. **Task completion validation**:
   - Check SQLite task store (`.forge/tasks.sqlite`): verify every task belonging to the milestone has `status == "completed"`. Fail closed with error listing incomplete tasks if any remain open.
2. **Worktree milestone resolution during task handoff**:
   - In `task_submit(stage="handoff")`, resolve the milestone file using the worktree path recorded in the active claim (`task_claims.worktree`): `Path::new(&worktree_path).join(&milestone_ref)`.
   - Call `std::fs::read_to_string` on that worktree-resolved path, guaranteeing that the receipt committed by the agent in its isolated worktree is what gets parsed and verified.
3. **Structured handoff receipt generation & duration computation**:
   - When a milestone transitions from `status: planned` to `status: active` (explicitly or upon the first `task_claim` of any task belonging to this milestone), write `started_at = <ISO-8601-NOW>` into the milestone frontmatter if not already present.
   - Compute development duration: `duration = completed_at - started_at` (formatted as e.g. `Xh Ym`). If `started_at` was missing, resolve activation time from the earliest task claim timestamp in SQLite.
   - Capture Git commit SHA (`git rev-parse HEAD` or `--commit <sha>`).
   - Record language-agnostic verification proof (command, passed/failed test counts, status).
   - Append or update the structured `handoff:` YAML block in the milestone document.
4. **Automated archival**:
   - Update frontmatter to `status: completed`.
   - Move the milestone file from `docs/milestones/<file>.md` to `docs/milestones/archive/<file>.md`.
   - Update SQLite store state.

---

### task: workflow-and-agent-instructions-alignment

```yaml
task_ref: workflow-and-agent-instructions-alignment
target: "Align AGENTS.md, docs/AGENTS.md, and reference documentation with milestone CLI commands and automated workflow"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - AGENTS.md
  - docs/AGENTS.md
  - docs/reference/
status: planned
depends_on:
  - milestone-init-and-scaffolding
  - milestone-list-and-show-inspection
  - structured-milestone-handoff-and-archival
```

1. **Update discovery & execution guidance in `AGENTS.md`**:
   - Instruct agents to start work by querying `contextunity-forge-mcp milestone list` and inspecting targets via `milestone show <id>`.
   - Instruct agents to scaffold new milestones using `milestone init --plan <path>`.
   - Instruct agents to close completed milestones via `milestone handoff <id>`, replacing manual file moves and manual status edits.
2. **Update reference documentation**:
   - Document `milestone init`, `list`, `show`, `handoff` in `docs/reference/cli.md` and `docs/reference/tasks.md`.
