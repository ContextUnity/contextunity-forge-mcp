# ContextUnity Forge MCP — Agent Router

## Read before work

- Read [documentation instructions](docs/AGENTS.md) before documentation changes.
- Read [architecture](docs/architecture/README.md) and [decisions](docs/adr/README.md) before structural changes.
- Read [src/AGENTS.md](src/AGENTS.md) before source code changes and module decomposition.
- Read [roadmap](docs/roadmap.md) and [milestones](docs/milestones/README.md) for admitted direction and commitments.
- Read [tests/AGENTS.md](tests/AGENTS.md) for test placement and authoring boundaries.

## Routes

- Runtime behavior and setup: [README.md](README.md) and [docs](docs/).
- Architecture and modularity: [src/AGENTS.md](src/AGENTS.md), [architecture](docs/architecture/README.md), and [decisions](docs/adr/README.md).
- Interfaces: [configuration and tools](docs/reference/README.md), [task operations](docs/reference/tasks.md), [ACDD navigation](docs/reference/acdd.md), and [CLI commands](docs/reference/cli.md).
- Operations: [runbooks](docs/runbooks/README.md).
- Planning: [roadmap](docs/roadmap.md) and [milestones](docs/milestones/README.md).
- Verification: [required gates](TESTS.md) and [test invariants](tests/AGENTS.md).
- Forge tool discovery and task operations: load the shared `contextunity-forge` skill.
- Follow the [Blackboard procedure](skills/contextunity-forge/SKILL.md#blackboard) for claim context, progress reports, and gate or agent handoffs.

When a symbol's unique name is already known, read it directly with
`forge_read(selector="SymbolName")`; add `path="..."` to restrict resolution.
Follow any source continuation to obtain the complete digest-verified body. Use
search when discovery or disambiguation is needed.

Use Blackboard `notes` for non-obvious technical context, not routine gate
status, passing tests, or SHAs that add no context. Before rejecting a review,
post structured `findings` with concrete remediation, then include the message
ID in the required typed rejection findings; see the linked procedure.

Use Forge MCP tools or the `contextunity-forge-mcp` CLI on the surface that provides the needed operation. Follow the active task profile and the `workflow_guidance` returned by task claim for gate sequence, proof policy, roles, models, review contours, and profile-specific behavior. Task builds and reviews run the scoped task seam test and strict Clippy (without forcing full unfiltered test suite execution during task iteration). Follow [TESTS.md](TESTS.md) for the exact task and milestone verification commands and count rules.

The embedded ACDD profile registers `rust-check-and-test` as strict Clippy followed by tests. For automated gates (`contract`, `build`), run `contextunity-forge-mcp task verify <task_id> --test <test_path::test_fn> [--seam <target>]` (or MCP `task_manage(action: "verify", ...)`), then submit the stage via `task submit <task_id> --stage <stage> --action pass` without manual evidence assembly. Delivery infers proof from the accepted candidate snapshot. Review and schema-defined gates provide their declared typed evidence payload.

<!-- acdd-forge:agents-verification:start -->
When authoring contracts or testing capabilities:
- **Table-driven contract matrices**: Use a single table-driven `#[test]` (`for case in cases`) — sharing one base workspace, cold index snapshot, or harness built once outside the loop — whenever testing homogeneous input, error, alias, spelling, hook, or boundary variants of the same contract or seam. Do NOT clone heavy `#[test]` functions (`Workspace::new`, `cold_build`, `InstallerHarness::new`, `McpTestHarness::new`, subprocess spawns) per variant.
- **Dedicated domain tests**: Author a separate `#[test]` in the owning domain module ONLY for a genuinely distinct domain contract, independent workflow, or orthogonal failure seam — never to split a matrix of variations of the same rule, and never by appending unrelated requirements into an existing workflow test.
- **Runtime-red contract proof**: Contract tests must compile cleanly and produce runtime assertion failures for `contract` gate proof.
- **Autonomous scope extension**: Agents should extend scope autonomously via `extend_scope` to include necessary files, fixtures, tests, and documentation within admitted roots to unblock implementation or verification without halting execution or requesting human permission.
<!-- acdd-forge:agents-verification:end -->

<!-- acdd-forge:contract-synthesis:start -->
## Architectural Blueprint Synthesis Gate (`blueprint_synthesis`)

The initial container of the milestone workflow is `blueprint_synthesis` (`synthesize` (role: `architect`) $\to$ `review` (role: `independent_reviewer`)).
Load and execute the [`contract-planning`](skills/contract-planning/SKILL.md) skill (with [`ADDON.md`](skills/contract-planning/ADDON.md)) during milestone authoring and blueprint synthesis:
1. Architectural Viability Audit: Critically evaluate design appropriateness and minimal complexity (Occam's razor).
2. ADR & Invariant Reconciliation: Reconcile proposals against `docs/adr/`, `docs/architecture/`, and active repository invariants before defining tasks.
3. Risk & Ambiguity Discovery: Probe for vague completion criteria, operational pitfalls, and failure semantics.
4. Constructive Improvement: Synthesize simpler, durable designs and leverage existing patterns before implementation begins.
5. Broad architectural exploration via Forge MCP (`code_map_*`, `ast_grep_search`, `forge_read`).
6. Minimal intervention principle (no duplicate registries or parallel tables).
7. Scope integrity (protected files `Cargo.toml`, `build.rs` are strictly off-limits).
8. Recording the architectural decision on `task_blackboard` before submission.
9. Establishing per-task blueprint slices (`proof.tasks[<task_ref>]`), cross-task seam boundaries (`incoming_seams`, `outgoing_seams`), and invariant specifications recorded in `gate_transitions` (`subject = <full_task_id>`) and injected automatically on `task claim`.
<!-- acdd-forge:contract-synthesis:end -->

## Documentation scan before handoff

Before milestone handoff, launch a documentation agent in the milestone worktree.
Give it the manifest, the final diff against the selected target base, the
accepted task evidence, and an explicit admitted documentation write scope.
Require it to load `agent-instructions`, read `docs/AGENTS.md` and affected owner
instructions, and reconcile behavior, CLI/MCP, configuration, workflows,
architecture, invariant descriptions, skills, and examples with live code/tests.
Update affected pages; explain when no update is needed. Preserve task-level
documentation co-evolution. Route source defects to their task owner and
architecture decision changes to owner approval. Post the scanned content binding,
updates, checks, and unresolved findings on the milestone blackboard. Submit the
documentation edits for independent review, then run the final checks in
`TESTS.md`. Repeat the affected scan if implementation changes afterward. The
coordinator launches the agent; this step adds no Forge gate or runtime launcher.

## Worktrees and Git permissions

Worktrees live at `.worktrees/<milestone-prefix>-<slug>` for milestones and `.worktrees/<milestone-prefix>-<task-slug>` for concurrent tasks.

### Permitted Git operations inside active milestone workflows

- Task commits:
  - With `auto_commit: true`, Forge automatically creates the delivery commit and records the receipt in SQLite; do not create duplicate commits.
  - With `auto_commit: false`, create exactly one scoped task commit containing task changes and the generated receipt after delivery.
  - Snapshot immutability: Never modify files during `deliver` stage or after `review`. Any edit triggers `TASK_CANDIDATE_MISMATCH`. If delivery fails, restore the worktree to the candidate snapshot before submitting rejection (`task submit --stage deliver --action reject`).
- History integration: Preserve accepted task proofs while integrating branches. Rebase, cherry-pick, merge, amend, and squash may change navigation SHAs without reopening completed tasks. Before handoff, obtain independent integration review and repository verification bound to the final content and explicit target base; follow the task and CLI references for the evidence file.
- Branch integration: Merge each delivered task branch into the milestone branch, delete the temporary task branch, and remove its worktree (`git worktree remove .worktrees/<milestone-prefix>-<task-slug>`).

### Linear Milestone Handoff Procedure

When every milestone task is completed with accepted proofs or identically cancelled:

1. **Absorb Polish & Emit Binding**:
   Run `contextunity-forge-mcp milestone handoff-context --target-base main <MILESTONE_ID>`.
   - Automatically absorbs uncommitted milestone adjustments (`.forge/**`, `AGENTS.md`, `Cargo.toml`, `docs/**`, `skills/**`, `.gitignore`, engine fixes).
   - Emits the content binding (`authority_digest`, `implementation_digest`, `target_base_digest`, `commit`, `builders`).

2. **Run Verification Commands**:
   Run `contextunity-forge-mcp milestone verify <MILESTONE_ID> --target-base main` after preparing the binding. It executes the applicable registered root command and records the transition and trace required for initial handoff. Run the separate `cargo test -p forge-tasks --lib` gate and follow [TESTS.md](TESTS.md#milestone-handoff-gate) for counts and any remaining required checks.

3. **Construct Integration Evidence File**:
   Write `.forge/integration-evidence.json` (ignored by Git) with:
   - `binding`: Exact binding object from `handoff-context` output.
   - `target_base`: Explicit target base (e.g. `"main"`).
   - `review`:
     - `worker_id`: Reviewer identity distinct from all `builders` in the binding (e.g. `"integration-reviewer"`).
     - `decision`: `"pass"`
     - `summary`: Passing integration review summary.
     - `scope_findings`: `[]`
     - `contract_findings`: `[]`
   - `verification`:
     - `command`, `exit_code`, `tests_passed`, `tests_failed`: Values returned by the successful `milestone verify` observation; the command and applicable counts must match its recorded trace.
     - `project_command`: Registered command ID from profile `commands:` (e.g. `"check_full_repo"`), NOT a raw shell string.
     - `lint_command`: Omit or specify a registered command ID from profile `commands:`.

4. **Execute Handoff**:
   Run:
   `contextunity-forge-mcp milestone handoff --evidence-file .forge/integration-evidence.json <MILESTONE_ID>`
   - Validates binding and evidence, moves milestone manifest to `docs/milestones/archive/`, prunes completed SQLite tasks, and outputs completion JSON.

5. **Commit Handoff Artifacts**:
   Stage the archived milestone and any admitted adjustments:
   `git add docs/milestones/`
   `git commit -m "docs(milestone): complete <MILESTONE_ID> handoff and archive authority"`

6. **Milestone Landing**:
   In the primary worktree (`main`), fast-forward the milestone branch, remove the milestone worktree, and delete the milestone branch:
   `git merge --ff-only <milestone-branch>`
   `git worktree remove .worktrees/<milestone-prefix>-<slug>`
   `git branch -d <milestone-branch>`

### Operations requiring explicit user approval

- Commits on `main`: Do not create Git commits directly on `main` without explicit user approval.
- Dangerous Git operations: History rewrites (`git reset --hard`, destructive rebases, force pushes), branch deletion outside task/milestone cleanup, and repository-level state clearing require explicit user approval.
- Remote publications: `git push`, tagging, releases, and credential publication require explicit user approval.

Do not run benchmarks without explicit user approval. Read [benchmark instructions](benchmarks/AGENTS.md) before benchmarking.

## Versioning and Release Notes

- **Mandatory Changelog Update**: Whenever bumping crate versions in `Cargo.toml` / workspace member manifests or preparing a release tag, update [`CHANGELOG.md`](CHANGELOG.md) in the same change with user-visible additions, behavioral changes, fixes, and migration/breaking notes under `## [<version>] - YYYY-MM-DD` (moving relevant entries out of `[Unreleased]`).
- **Workspace Version Sync**: Keep `Cargo.toml`, `crates/*/Cargo.toml`, installation guides (`INSTALL.md`), and workspace contract tests synchronized to the same version.

## Launching Codex agents (Antigravity Managed Background Task)

Launch Codex agents (`codex exec` / `codex exec resume`) directly through the native platform tool `run_command` with `WaitMsBeforeAsync: 500`:

```json
{
  "CommandLine": "codex exec resume -c model=\"gpt-6.1-sol\" -c model_reasoning_effort=\"medium\" <SESSION_ID> \"<PROMPT>\"",
  "Cwd": "/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/<milestone-prefix>-<task-slug>",
  "WaitMsBeforeAsync": 500,
  "toolAction": "Starting Codex Sol builder agent",
  "toolSummary": "Start Codex Sol builder for task"
}
```

For a new session without an existing `<SESSION_ID>`, run `codex exec -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" "<PROMPT>"` with the same `WaitMsBeforeAsync: 500` parameter.

### Review execution and findings feedback loop

1. **Review Stage (`model_reasoning_effort="xhigh"`)**:
   - Run the independent reviewer agent with maximum reasoning effort:
     `codex exec -c model="gpt-6.1-sol" -c model_reasoning_effort="xhigh" "<REVIEW_PROMPT>"`
   - Use an independent `worker_id` (e.g. `codex-sol-reviewer`) distinct from the builder worker.
   - Reviewer operational guidance, performance invariants, and evaluation criteria are defined in the active profile (`.forge/acdd/profile.yaml`).
2. **Builder feedback loop (`codex exec resume <BUILDER_SESSION_ID>`)**:
   - Always route review findings, rejection details, and remediation tasks back into the **same builder session**:
     `codex exec resume -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" <BUILDER_SESSION_ID> "<REVIEW_FINDINGS>"`
   - This preserves the builder's complete context, file history, and reasoning continuity without cold restarts.

### Why this execution pattern is required

1. **`WaitMsBeforeAsync: 500` (Managed Background Task)**:
   - The platform spawns the command directly in the Linux shell (`bash -c`).
   - When the process runs longer than 500 ms, the platform kernel automatically promotes it to a Managed Background Task and displays the live status bar (`[HH:MM:SS] codex exec ... running`).
2. **Reactive wakeup (Zero-Polling)**:
   - Do not poll task status in a loop. Stop calling tools at the end of the turn when waiting on the background agent.
   - When the process finishes (success or error), the platform event bus delivers `<SYSTEM_MESSAGE> Task id "task-XXXX" finished with result...` and reactively wakes the coordinator for the next step.
3. **Real-time log streaming**:
   - The platform streams process output in real time to `.system_generated/tasks/task-XXXX.log`, which can be inspected on disk without losing context.
