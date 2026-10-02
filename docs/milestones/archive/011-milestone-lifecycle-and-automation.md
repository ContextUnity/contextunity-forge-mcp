---
id: m-milestone-lifecycle-and-automation
title: Milestone CLI automation, structured handoff, and archival
doc_type: contract
status: completed
depends_on:
- m-repository-task-lifecycle:completed
owners:
- src/cli/
- src/engine/
- src/core/
- docs/
- tests/
invariants:
- 'INV-MILESTONE-CLI: Milestone lifecycle operations (list, show, init, handoff) are CLI-only tools without polluting the four-tool MCP contract.'
- 'INV-STRUCTURED-HANDOFF: Milestone handoff generates a typed YAML receipt recording duration, git commit, and language-agnostic verification proof.'
- 'INV-AUTOMATED-ARCHIVAL: Completing a milestone automatically moves the file to docs/milestones/archive/ and syncs task state.'
- 'INV-SEAMLESS-SCAFFOLDING: Milestone init creates the next sequentially numbered milestone from a plan with pre-filled contracts and agent guidance.'
- 'INV-STARTED-AT-SEMANTICS: Milestone started_at timestamp remains empty while status is planned; it is stamped with NOW() when activated or upon the first task_claim of the milestone to ensure accurate active development duration.'
- 'INV-WORKTREE-RECEIPT-RESOLUTION: Task handoff receipt verification resolves the milestone file within the claimed task worktree directory (task_claims.worktree + milestone_ref), verifying the exact file authored by the claiming agent.'
started_at: 2026-10-02T04:07:31+00:00
handoff:
  completed_at: 2026-10-02T06:44:54.755904827+00:00
  duration: 2h 37m
  commit: 86911d3b32694453434900770d1159aba7f28dac
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 437
    tests_failed: 0
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
status: completed
receipt:
  commit: b06be9cdc3f1180068ffb70c400d7bff726dc774
  contract_revision: 3
  passed_at: "2026-10-02T04:44:16Z"
  evidence:
    command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings
    result: passed
    artifacts: []
  review:
    decision: pass
    evidence_ref: .forge/011-init-review.md
    contours:
      paths:
        applicable: true
        evidence: "CLI root selects the milestone directory; plan reads are confined to the root; active and archive paths participate in numbering and ID checks; new files use create_new."
      claims:
        applicable: true
        evidence: "Contract revision 3 behavior is present for explicit and automatic numbering, plan and stdin content, started_at, and actionable sync and claim guidance; CLI seam tests cover these outputs."
      concurrency:
        applicable: false
        evidence: "The admitted init contract does not specify serialization of simultaneous CLI invocations; create_new prevents overwriting the same target path."
      project_isolation:
        applicable: true
        evidence: "The selected workspace root bounds milestone scanning, output, and plan input; confined_path rejects plan paths outside it."
      administration:
        applicable: true
        evidence: "Milestone init is routed through the CLI command tree only; the MCP task tool surface remains unchanged."
  decision: pass
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
status: completed
receipt:
  commit: efb169d0a2e8c7f24dbb171b3c8625ce027b5ad0
  contract_revision: 2
  passed_at: "2026-10-02T05:41:47Z"
  decision: pass
  evidence:
    command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings
    result: passed
    artifacts: []
  review:
    decision: pass
    evidence_ref: .forge/011-list-review.md
    contours:
      paths:
        applicable: true
        evidence: "Numbered Markdown filtering excludes README guides; default list omits archived paths before parsing; show selects by prefix or frontmatter ID before task YAML parsing."
      claims:
        applicable: true
        evidence: "Rev2 list and show fields, status/archive filters, SQLite completion ratios, and full document output are exercised through the CLI seam test and verified in current source."
      concurrency:
        applicable: false
        evidence: "This task defines read-only CLI inspection and no concurrent mutation or serialization guarantee; no shared state is changed by list or show."
      project_isolation:
        applicable: true
        evidence: "Milestone repository/project overrides are validated and each document reads task state from its matching SQLite project namespace; the seam test verifies an alternate namespace ratio."
      administration:
        applicable: true
        evidence: "List and show are CLI subcommands only and do not extend the four-tool MCP task surface; invalid status and ambiguous selectors fail with errors."
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
status: completed
receipt:
  commit: 5bacfd378892f20b978492013631875010960da6
  contract_revision: 2
  passed_at: "2026-10-02T06:26:58Z"
  evidence:
    command: cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings
    result: passed
    artifacts: []
  review:
    decision: pass
    evidence_ref: .forge/011-handoff-review.md
    contours:
      paths:
        applicable: true
        evidence: "Task receipt verification reads the milestone from the active SQLite claim worktree using a confined relative path; handoff creates the archive path, updates references, and removes the active file."
      claims:
        applicable: true
        evidence: "Handoff requires completed SQLite task status, computes duration from started_at or earliest claim, and writes completed_at, duration, commit, and typed language-neutral verification fields; CLI tests exercise these contracts."
      concurrency:
        applicable: true
        evidence: "Archive creation uses create_new; SQLite relocation uses an immediate transaction that checks task status, old reference, and the expected task ID set before committing."
      project_isolation:
        applicable: true
        evidence: "Milestone repository/project identity selects the matching task store namespace, and claim worktree path confinement protects the receipt read."
      administration:
        applicable: true
        evidence: "Milestone handoff is a CLI command and leaves the four MCP task tools unchanged; task claim activation and handoff update state through existing task-store seams."
  decision: pass
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
status: completed
receipt:
  commit: 94222a476d6d77d495f8b2e8be78ded8b8072f70
  contract_revision: 2
  passed_at: "2026-10-02T06:42:17Z"
  evidence:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    result: passed
    artifacts: []
  review:
    decision: pass
    evidence_ref: .forge/011-workflow-review.md
    contours:
      paths:
        applicable: true
        evidence: "AGENTS.md and docs/AGENTS.md route milestone work through CLI commands; docs/reference/cli.md and docs/reference/tasks.md own the documented interface and link to each other."
      claims:
        applicable: true
        evidence: "The reference describes init, list, show, handoff, SQLite progress, started_at, worktree receipt reading, archival, and language-neutral verification fields in terms matching candidate source."
      concurrency:
        applicable: false
        evidence: "This task changes documentation only; its contract admits no concurrent state transition or serialization guarantee."
      project_isolation:
        applicable: true
        evidence: "Task reference guidance describes shared task-store configuration, project identity, and receipt resolution from the claimed worktree without introducing a cross-project shortcut."
      administration:
        applicable: true
        evidence: "The guidance makes milestone lifecycle CLI-only and retains the four flat MCP task tools; agent instructions describe task receipts, quality gates, approval for commits, and archival."
  decision: pass
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
