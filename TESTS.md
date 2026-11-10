---
title: "Tests and Verification"
doc_type: guide
---

# Tests and Verification

Read this file for repository-wide test gates. Read [tests/AGENTS.md](tests/AGENTS.md)
for test placement, authoring boundaries, public-seam requirements, and benchmarking
isolation.

## Task-level proof

Run the focused test command required by the admitted task contract (e.g. `cargo test --test <target> <seam_filter>` or scoped test module). Record the exact command, exit code, passing count, and failing count in the active `build` and `review` evidence. This focused test proof uses the narrowest real production seam that proves the task without forcing slow, unrelated suite execution during task iteration.

For Rust task builds and reviews, run strict Clippy alongside the focused test:
`cargo clippy --all-targets --all-features -- -D warnings`. Record its exact command and exit code with the build evidence; Clippy contributes no test count.
The embedded ACDD profile registers the scoped verification command:
`cargo clippy --all-targets --all-features -- -D warnings && cargo test`.
Claim context supplies `workflow_guidance.scoped_commands` with the contract seam and indexed covering-test targets; use the scoped command matching the task proof (with test filter) and record its exact string. Full unfiltered suite execution (`cargo test --all-targets`) is deferred to the milestone handoff gate.

## Milestone handoff gate

Before `milestone handoff`, run each command from the repository root:

1. `cargo test --test commitment_integrity`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --all-targets`
4. `cargo test -p forge-tasks --lib`

The root package's `--all-targets` run does not include the `forge-tasks`
member's library unit tests. Record the separate member-library result alongside
the root suite result.

Prepare the binding with `milestone handoff-context <id> --target-base main` and
obtain independent integration review against that exact final content and base.
Follow the documentation-agent scan in [AGENTS.md](AGENTS.md#documentation-scan-before-handoff)
before final verification. Handoff context commits admitted working-tree polish
before producing the review binding. On that content, run
`milestone verify <id> --target-base main` to execute the first applicable root
command and persist its observation. The current repository profile's
`check_full_repo` runs commands 1–3 above; run command 4 separately. Confirm
the observed command still covers commands 1–3, and run any missing checks
separately if the profile changes. Obtain fresh independent review.

For initial handoff, copy the successful observation's `command`, `exit_code`,
`tests_passed`, and `tests_failed` into the corresponding `verification` fields
of `IntegrationEvidence`. Forge matches the command and applicable counts to
its latest trace; do not replace its observed count with a deduplicated count.
Separately report the unique passed-test count from `cargo test --all-targets`,
counting the standalone commitment run only once and excluding Clippy, and report
the member-library count without adding it to the trace-bound value. Supply the
record to `milestone handoff <id> --evidence-file /tmp/handoff-evidence.json`;
see the [record fields](docs/reference/tasks.md#milestone-cli-lifecycle).
For source polish, select its registered project verification command through
`verification.project_command`; handoff verifies or reuses the recorded observation.
Select the registered lint command explicitly through `verification.lint_command`;
handoff verifies or reuses its recorded successful observation. These observations do not
replace the mandatory commands or their count rules. Changed content or authority
requires fresh final review and verification.

Use `test-suite-refactor` only when the admitted change splits, replaces,
deduplicates, or removes a typed test suite. It is not a general final-verification
step.

## Test authoring and fixture invariants

To prevent test-suite bloat, redundant execution storms, and synthetic masking, all test suites MUST adhere to these architectural invariants:

1. **Anti-Fake-Gating & Shared Snapshot Setup**:
   - `#[ignore]` gating (`FORGE_TESTS_GIT=1`) is strictly forbidden as a substitute for optimizing redundant test setups.
   - Matrix tests verifying rejections, invalid receipts, or failure modes MUST NOT re-run multi-gate task/milestone lifecycles from scratch for each scenario row.
   - Establish the base state ONCE (e.g. delivered milestone state) and use lightweight branching (`git switch -c`), worktree branching, or in-memory SQLite snapshots to mutate only the single condition under test per scenario. All rejection scenarios must execute in seconds, not minutes.
   - Do NOT use `.filter(|scenario| git_matrix != ...)` to sweep unoptimized multi-minute loops into the ignored lane while leaving the underlying test unoptimized.

2. **Canonical Fixtures & Builders Over Inline Heredocs**:
   - Do NOT copy-paste raw, multi-line YAML frontmatter and markdown milestone documents (`let source = "---\nid: ...";`) across test functions.
   - Shared fixture variations belong in canonical template files under `tests/acdd/fixtures/` or structured fluent builders (`MilestoneFixtureBuilder`). Tests must reference templates or configure variations via typed builder methods, keeping test files maintainable and concise.

3. **In-Process Engine Seams Over OS Process Spawning**:
   - Integration tests proving milestone and task lifecycle logic must invoke the in-process Rust API directly (`milestone_engine::handoff`, `tasks::manage`, `TasksStore`).
   - OS-level binary execution (`std::process::Command::new(env!("CARGO_BIN_EXE_..."))`) is restricted exclusively to dedicated CLI smoke tests (1–2 per subcommand) to verify CLI argument parsing and exit codes. Never drive domain matrix permutations through subprocess spawning.

4. **In-Memory Store First**:
   - Pure state-machine logic, gate transitions, DAG status queries, and invariant checks must run against in-memory SQLite stores (`TasksStore::open_in_memory(":memory:?cache=shared")`) in < 5ms without disk files or external Git processes.

5. **Universal Declarative Test Harnesses (`<Domain>Harness`)**:
   - Integration tests must express domain intent, not procedural plumbing.
   - Individual test functions MUST NOT interleave manual Git subprocess scripting (`git checkout`, `git commit`, `git worktree`), raw SQLite mutations (`INSERT INTO task_blackboard`), or 80-line JSON evidence payloads.
   - Every major cluster must provide a cohesive domain harness (`MilestoneHarness`, `TaskHarness`, `McpTestHarness`, `ExtractorHarness`).
   - All environment lifecycle mechanics, Git commits, evidence synthesis, and blackboard updates must be encapsulated within the respective harness. Keep each test's setup, action, and assertions clear.

6. **Seam-Preserving Functional Decomposition Over Monolithic Mega-Tests**:
   - Functional decomposition MUST NOT degrade production seam coverage into mockist micro-unit tests. Every test must execute genuine component integration pathways end-to-end (real SQLite, real Git commits, real Linker ASTs).
   - Split tests that chain distinct, unrelated scenarios, such as the 1,200-line extractor and 500-line lifecycle monoliths. Treat 50 lines as a prompt to review cohesion and readability, not as an acceptance limit.
   - Use separate, descriptively named `#[test]` functions for distinct capabilities and workflows. Related input variations may share a table when they exercise the same production seam.

7. **Zero-Disk In-Memory Syntax & AST Invariant**:
   - Unit tests for AST extraction, syntax parsing, language semantics, and framework manifest resolution MUST execute purely in-memory (`ExtractorHarness::for_lang(Rust).extract(code)` or in-memory SQLite connections).
   - They MUST NOT allocate physical filesystem tempdirs (`Workspace::new()`), initialize Git repositories, or write physical files to `/tmp` or `.forge/` unless physical filesystem boundary behavior (such as file locking or symlinks) is the explicit subject of the test.

8. **In-Process MCP Routing Over Subprocess Transports**:
   - Protocol and tool tests in `tests/mcp/` and `tests/acdd/mcp.rs` MUST dispatch JSON-RPC requests directly through the in-process server handler (`server.handle_request(&req)`).
   - Spawning background child processes (`StdioClient::new()`, `mkfifo`, subprocess pipes) is strictly restricted to exactly 1–2 transport smoke tests in `tests/mcp/`. Never drive matrix permutations, tool freshness checks, or response compaction through OS child processes.

9. **Plan & Workflow Orchestrator Testing Invariant (`INV-PLAN-WORKFLOW-HARNESS`)**:
   - All tests for structured plans (`docs/plans/*.md`) and standalone workflows (`docs/workflows/*.yaml`) in Milestone AC-003 MUST use `PlanHarness` and `WorkflowHarness` with canonical static templates under `tests/acdd/fixtures/plans/` and `tests/acdd/fixtures/workflows/`.
   - Ad-hoc markdown/YAML heredoc copy-pasting, direct procedural bash-like scripting, and external CLI subprocess spawning are strictly banned for plan and workflow orchestrator test suites.

10. **Profile-Driven Universal Lifecycle & Worker Independence (`INV-TEST-PROFILE-DRIVEN-LIFECYCLE`)**:
    - Task integration and lifecycle tests must NOT hardcode private gate lists (`&["contract", "build", "review", "deliver"]`).
    - Task advancement must be driven dynamically via the task's active `GateProfile` (`harness.profile().executable_gates()`, `claim_next()`, `advance_to()`, `advance_to_completion()`).
    - The test harness automatically derives isolated synthetic sessions per simulated worker (`test-session-{worker}`) to make in-process tests immune to ambient host environment variables.
    - Dedicated integration tests must verify that an independent gate rejects the accepted predecessor's `worker_id` with `TASK_REVIEW_NOT_INDEPENDENT`, regardless of session metadata, while a distinct worker is not vetoed by missing or equal session metadata.
    - Runner tests must separately verify distinct session identities and fingerprints, process isolation, and retained session reuse on rejection.

## Declarative Seam Testing via `<Domain>Harness`: Agent Authoring Guide

When writing or extending tests in this repository, agents MUST follow the **Production Seam Test** architecture driven by declarative `<Domain>Harness` instances.

> [!NOTE]
> For the complete architectural specification, boundary matrix, and Before/After code patterns, read [Test Harness and Production Seam Architecture](docs/architecture/test-harness.md).

### The Seam Test Mental Model
- **Real Production Pipelines (No Mockist Fakes)**: A Seam Test exercises the genuine production path end-to-end (live SQLite schemas, real Git tree hashes, real Engine gates, and AST Linkers). Never mock collaborating subsystems.
- **No Procedural Plumbing in Tests**: Tests express business contracts and verification intent. All repetitive plumbing (creating temp workspaces, `git commit`, `git worktree`, raw SQL `INSERT`, and 80-line JSON evidence builders) is encapsulated inside the respective `<Domain>Harness`.
- **One Clean Scenario per `#[test]`**: Each test function verifies one independent functional scenario on a clean baseline or RAM snapshot. Keep related input variations together when a table makes the contract clearer.

### Available `<Domain>Harness` Implementations
| Harness | Subsystem / Scope | Canonical Fixtures | Key Production Seams |
| :--- | :--- | :--- | :--- |
| `MilestoneHarness` | Milestone graph, DAG dependencies, verification evidence, handoff | `tests/acdd/fixtures/milestones/` (`linear.md`, `branching_deps.md`, `cancellation.md`) | `milestone_engine::handoff`, `milestone_engine::handoff_context` |
| `TaskHarness` | Task state machine, claim, submit, profile-driven lifecycle, blackboard | In-memory SQLite for state-only cases; Git workspaces for persisted lifecycles | `tasks::claim`, `tasks::submit`, `tasks::manage` |
| `PlanHarness` | Plan fixtures, workspace materialization, and snapshot inspection | `tests/acdd/fixtures/plans/` (`linear.md`, `dag.md`) | `docs::extract_with_details`, plan runner |
| `WorkflowHarness` | Root-gate profile validation and workspace materialization | `tests/acdd/fixtures/workflows/` (`simple.yaml`, `rejection_routing.yaml`) | `profile::validate_profile_document`, workflow runner |
| `ExtractorHarness` | Syntax parsing, AST extraction, fact emission (zero disk I/O) | In-memory code snippets | `languages::rust::extract`, `languages::python::extract` |
| `McpTestHarness` | MCP JSON-RPC protocol, tool routing, response compaction | Real Workspace files, SQLite, and workspace-local extraction cache | Production RMCP server over an in-process duplex stream |

Run the plan and workflow harness checks with
`cargo test --test acdd acdd::harness::`. These checks exercise Markdown
extraction and the shared gate-profile schema from ADR 0021; they do not prove
structured plan parsing, DAG execution, role dispatch, or persistence effects.
Runner tests exercise those behaviors separately with materialized fixtures.

---

### The Three Canonical Authoring Scenarios

#### Scenario 1: Extending an Existing Capability (Input Variations & Edge Cases)
When verifying a new edge case, diagnostic error code, or parameter variation on an existing seam:
- **Do NOT** copy-paste an entire 100-line test function.
- If the test is table-driven, add **one row** to the existing case table:
  ```rust
  for (scenario, expected_error) in [
      ("stale_evidence", HandoffError::EvidenceStale),
      ("missing_subtask_evidence", HandoffError::SubtaskEvidenceMissing),
  ] {
      // parameterized assertion
  }
  ```
- If testing a mutation or rejection condition, use a 1-line declarative mutator on the harness:
  ```rust
  #[test]
  fn handoff_rejects_when_verification_fails() {
      let mut h = MilestoneHarness::linear("m-reject");
      h.complete_all_tasks();
      let err = h.handoff_with_mutated_evidence(|ev| ev.verification.exit_code = 1).unwrap_err();
      assert_matches!(err, HandoffError::VerificationFailed { exit_code: 1, .. });
  }
  ```

#### Scenario 2: Authoring a New Contract Invariant (New Feature / Rule)
When a task introduces a distinct new behavioral contract or fail-closed invariant:
- Author a dedicated, descriptively named `#[test]` function in the appropriate domain module.
- Load a canonical fixture via the harness and express only the distinct domain preconditions, action, and assertion:
  ```rust
  #[test]
  fn plan_document_sections_preserve_the_declared_type() {
      let h = PlanHarness::linear();
      assert!(!h.sections.is_empty());
      assert!(h.sections.iter().all(|section| section.doc_type == "plan"));
  }
  ```
  This example exercises production Markdown extraction. Separate runner tests
  cover structured plan execution and subtask delivery guards.

#### Scenario 3: Connecting an Existing Seam to a New Consumer
When a new subsystem emerges (e.g. a consumer of `PlanHarness` or `TasksStore`):
1. **Extend the Harness API additively**: Once the production consumer exists,
   add its invocation to the domain harness so tests share the real wiring.
2. **Author the Integration Seam Test**: Instantiate producer state via the
   harness, execute the consumer, and assert observable state transitions across
   both components without mocking either collaborator. Harness checks alone
   prove document extraction and gate-profile validation; exercise the runner
   separately to verify execution and persistence.

---

### Non-Negotiable Test Authoring Rules (Anti-Pattern Checklist)
- ❌ **NEVER** write `let source = "---\nid: ...";` in test functions (use `tests/acdd/fixtures/` or typed builder).
- ❌ **NEVER** invoke `std::process::Command::new("git")` or `Command::new(env!("CARGO_BIN_EXE_..."))` in domain tests.
- ❌ **NEVER** execute raw SQL `store.connection.execute("INSERT INTO ...")` in domain test functions when a reusable store/harness primitive exists.
- ❌ **NEVER** dump static non-domain functions onto `<Domain>Harness` (e.g. `McpTestHarness::seed_git` or `McpTestHarness::edge_rows`) or alias harness types (`use McpTestHarness as Client` / `as Mcp`).
- ❌ **NEVER** inline a single-caller helper as a 100-line `(|| -> Result<_> { ... })()` IIFE block returning a one-off `*Observation` struct, or leave zero-method `*Observation` struct shells in `harness.rs`.
- ❌ **NEVER** run full-DB dumps (`persisted_state`) or $O(N^2)$ sequential cold index rebuilds inside per-row rejection/delta loops when lightweight checks or batched file updates prove the same contract.
- ❌ **NEVER** synthesize 40–80 lines of raw JSON evidence manually (use `h.default_evidence()` and declarative mutators), or hide setup bugs behind silent `"unavailable"` digest fallbacks in happy-path fixture helpers.
- ❌ **NEVER** combine unrelated workflows into 300-line mega-tests, or author multi-hundred-line meta-test suites for test-only helpers.
- ❌ **NEVER** mechanically chop a cohesive single-domain test file (e.g. 800–1,600 lines testing one tool or language surface) or hide test assertions in `*Harness` helpers to chase an arbitrary line count. Decompose multi-subsystem monoliths (>1,500–2,000 lines mixing unrelated contracts such as `tests/acdd/mcp.rs`, `tests/mcp/context.rs`, `tests/mcp/tool_evolution/mcp_boundaries.rs`) by domain cohesion, keeping `tests/commitment_integrity/*` under `< 500` lines since its submodules are already partitioned by domain.

## Test lanes

Run the default repository suite with `cargo test --all-targets`. All current
registered scenarios, including Git/history and scale cases, stay in this lane.
The current Rust tree has no `#[ignore]` tests or `FORGE_TESTS_GIT` /
`FORGE_EXTENDED_TESTS` test switches, so there is no separate ignored or
environment-gated lane. Record passed, failed, and ignored counts; the default
run currently reports zero ignored tests.

Run the `forge-tasks` member-library suite separately with
`cargo test -p forge-tasks --lib`; use
`cargo test -p forge-tasks --lib handoff_accepts_completed_reviewed_shared_scope`
for focused development on its native handoff seam.

The milestone tests live in `tests/acdd/milestones/`. Run their default cases
with `cargo test --test acdd milestones`; select an individual module with
`cargo test --test acdd milestones::<module>::`.
