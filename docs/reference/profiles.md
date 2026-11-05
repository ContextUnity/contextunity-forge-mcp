---
title: "ACDD Profiles Reference"
doc_type: api
---

# ACDD Profiles Reference

An **ACDD Profile** (`profile.yaml`) defines the governance model for a task or milestone: its ordered verification gates, proof evaluation policies, review contours, worker role assignments, and recommended AI models.

By separating workflow governance from code and task specifications, profiles allow teams to enforce strict multi-agent compliance (e.g. mandatory independent reviewer separation, TDD seam-first gates, and SHA-256 anti-tampering verification).

The embedded profile uses three subagent roles: `contract_author` (`sol-6.1:high`), `builder` (`gpt6-luna:xhigh`), and `independent_reviewer` (`sol-6.1:xhigh`). The contract author targets a draft to `build` on the task blackboard. The independent reviewer keeps the same worker session through `review` and `deliver`, remaining distinct from the builder. Claim guidance includes `NO_PREFLIGHT_SUBAGENTS` so gates start without preflight agents.

> [!NOTE]
> Contract, command, and seam-test passes require claim-bound execution observations. Seam-test references require individual passed-test observations, not only counts; see [supported commands](tasks.md). Milestone handoff, including archived retries, requires a closed root verification checkpoint matching the final candidate and target base. These checks do not implement the deferred lifecycle-kernel or immutable historical-source redesigns. Native headless dispatch is planned for AC-003.

---

## Profile Loading Precedence

Forge resolves the active profile in the following order:

1. **Task specification**: Pinned in task metadata as `acdd_profile: "<path>:<sha256_prefix>"`.
2. **Milestone frontmatter**: Declared in milestone YAML frontmatter as `acdd_profile: "<path>:<sha256_prefix>"`.
3. **Workspace adapter**: Referenced in `forge-mcp.yaml` under `acdd_profile: "<path>"`.
4. **Repository default**: File present at `.forge/acdd/profile.yaml`.
5. **Embedded default**: Built-in default workflow (`acdd.default.yaml`).

> [!NOTE]
> Linked workspaces (`linked_workspaces`) do not override the parent ACDD profile. Only the active workspace root governs the task profile.

---

## Profile Structure

Profiles merge with the embedded or workspace base by default. Set `inherit: none`
to compile a clean-room profile from its own declarations, without inheriting the
base roles, gates, commands, policies, proofs, or guidance entries. The profile
must then declare the roles and gates it needs. An explicit `commands: {}` clears
an inherited command catalog; omit `commands` to keep the inherited catalog. A
profile with an empty catalog must not declare gates that require command proof.

A complete profile YAML consists of the primary sections shown below:

```yaml
# .forge/acdd/profile.yaml
version: 2

roles:
  contract_author:
    mode: subagent
    reuse_on_reject: true
    models:
      - 'sol-6.1:high'
    guidance: "As contract author, define the production-seam contract and runtime-red proof."

  builder:
    mode: subagent
    reuse_on_reject: true
    models:
      - 'gpt6-luna:xhigh'
    guidance: "Implement the approved contract inside declared allowed_write_scope."

  independent_reviewer:
    mode: subagent
    reuse_on_reject: true
    models:
      - 'sol-6.1:xhigh'
    guidance: "Review independently, then claim and submit delivery in the same reviewer session."

commands:
  rust-check-and-test:
    by_pattern:
      "**/*.rs": "cargo clippy --all-targets --all-features -- -D warnings && cargo test"
    description: "Run strict Rust checks followed by the repository's test suite."
  check_full_repo:
    by_pattern:
      "**/*.rs": "cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets"
    description: "Run milestone handoff verification commands from TESTS.md."

guidance:
  task.contract: "Define a production-seam proof and keep the task contract inside admitted scope."
  task.code: "Implement the accepted seam with the smallest change that satisfies the contract."
  task.review: "Review the candidate against scope, contract, concurrency, isolation, and administration evidence."
  hitl.pro: "HITL level PRO: allow internal helpers and non-breaking implementation changes; escalate public APIs."
  contour.paths:
    description: "Verify changes stay within the task's admitted write perimeter."
    criteria:
      - "Compare every changed file with the task's initial scope."
  contour.claims:
    description: "Verify each accepted contract claim is implemented."
    criteria:
      - "Trace every invariant to the production code path."

policies:
  default_review:
    independent_worker: true
    require_all_contours: true
    audit_extended_scope: true

gates:
  - id: contract_synthesis
    role: architect
    guidance: [milestone.synthesis, hitl.pro]
    expected_failure:
      exit_codes: [101]
    save:
      pre: snapshot
      post: snapshot

  - tasks:
      gates:
        - id: contract
          role: contract_author
          guidance: [task.contract, hitl.pro]
          proof: contract
          save: snapshot

        - id: build
          role: builder
          guidance: [task.code, hitl.pro]
          command: rust-check-and-test
          save:
            pre: snapshot
            post: snapshot

        - id: review
          role: independent_reviewer
          guidance: [task.review, hitl.pro]
          policy: default_review
          contours: guidance.contours
          independent_from: build
          fail: build
          loop:
            scope: build
            symbols: contract
          save:
            immutable: true

        - id: deliver
          role: independent_reviewer
          guidance: [hitl.pro]
          receipt: true
          need: [review]
          independent_from: build
          fail: build
          save:
            immutable: true
            post: commit
            git_commit_note: telemetry

  - id: verify
    command: check_full_repo
    max_attempts: 2
    pass: documentation
    fail: polish

  - id: polish
    role: architect
    guidance: [milestone.polish, hitl]
    pass: milestone_review
    save:
      pre: snapshot
      post: snapshot

  - id: milestone_review
    role: independent_reviewer
    policy: default_review
    guidance: [milestone.review]
    contours: guidance.contours
    independent_from: polish
    pass: verify
    fail: polish
    save:
      immutable: true

  - id: documentation
    role: doc_writer
    when: doc
    guidance: [milestone.doc]
    pass: handoff

  - id: handoff
    receipt: true
    absorb: true
    save: commit
```

Workflow guidance selects `rust-check-and-test`, the canonical strict Clippy-plus-test command. Profiles may define
`scope_args_by_pattern` as project-relative path patterns mapped to command
arguments. The most specific matching pattern wins, and `{file_stem}` expands
to the matched path's filename without its extension. For each configured
command, claim context substitutes those arguments for `{scope_target}` or
appends them when the command has no placeholder. If no pattern matches, the
ordinary path is used, which keeps non-Cargo commands such as `pytest` and
`ruff` scoped to their file paths. The repository workspace profile maps Rust
source files to Cargo library or binary targets and integration test files to
their actual Cargo test target. Claim context derives
`workflow_guidance.scoped_commands` from the indexed contract seam and covering
tests, and provides `expected_evidence_template` bound to the active task,
stage, revisions, worker, and worktree.

Detailed and `by_pattern` command definitions accept an optional `output`
mapping. Its optional `counts` mapping contains `passed` and `failed` regular
expressions. `require_counts` defaults to `true`. `noise` and
`errors` are lists of regular expressions, and `seam_ref_pattern` is an optional
regular expression. The optional `tests` mapping defines declarative rules for
extracting individual test outcomes and source identities:
`passed` (required pattern matching an individual passed test and capturing its name/ref in group 1),
`failed` (optional pattern matching an individual failed test in group 1),
`ignored` (optional pattern matching an individual skipped/ignored test in group 1), and
`target` (optional pattern extracting the test target/entrypoint source file in group 1).
Count, seam, and individual test patterns extract capture group 1; the profile
validator rejects invalid expressions and extraction patterns without that group.
Counts are summed across matching summaries. When a configured passed count is
present and the failed pattern has no match, the failed count is zero. Missing
required counts, malformed integers, overflow, and reported failures reject proof.
Set `require_counts: false` for linters and documentation checks whose zero exit
code proves success without a test count. Configure output rules to describe command counts and seam references.

The command runner parses complete output before filtering `noise` lines, prioritizing
`errors` lines, and truncating diagnostics. Durable claim-bound facts retain the
counts, configured seam reference, expected failure signature, and output rules
without requiring raw log files. The intended proof binding invalidates reuse when the candidate, claim, command identity,
or output rules change. Reuse must also require the ordered
execution traces to match the complete current command plan resolved from the
admitted scope and extensions, including argument expansion and deduplication.
Failure signatures use complete-output facts when available.
The embedded pattern commands carry
declarative output rules for Rust, Python, and JavaScript test summaries.

`limits.diagnostic_inline_bytes` defaults to 4096 and accepts 1..=65536 bytes.
Passing commands and matching contract failures produce compact summaries without
logs. Short failures remain inline; larger cleaned diagnostics retain head and
tail excerpts and link the raw log under the claimed worktree's
`.forge/task-traces/`. `limits.max_command_timeout_seconds` defaults to 1800 and
bounds positive runtime timeout overrides, including requests that reuse cached
proof. Each command's `timeout_seconds` supplies its ordinary timeout (300 seconds
when omitted). See [task verification](tasks.md) for CLI and MCP operations.

---

## Key Concepts

### 1. Gates and Proof Kinds

Each gate declares a stable identifier (`id`) and a proof specification (`proof`):
- `contract`: Verifies a red seam test (default `seam-test-first` expecting non-zero exit code) or invariant baseline (`direct-proof`).
- `command`: Executes and verifies a registered CLI command (must exit with code 0).
- `review`: Evaluates cognitive review contours (`contours: guidance.contours`) and enforces worker separation (`independent_from`).
- `delivery`: Terminal gate that writes the durable receipt into SQLite and milestone Markdown.
- `scheme`: A custom recursive JSON schema validation under `proof: { scheme: ... }`.

### 2. Review Focus Lenses (`contours: guidance.contours`)

Review contours are declared directly under `guidance:` using the `contour.<name>` prefix as structured entries with `description` and operational `criteria`:

```yaml
guidance:
  contour.paths:
    description: "Verify changes stay within the task's admitted write perimeter."
    criteria:
      - "Compare every changed file with the task's initial scope."
```

Review gates bind explicitly to these lenses using `contours: guidance.contours`. The review proof engine and `workflow_guidance` extract the structured criteria to drive reviewer checklists and evaluate evidence.

### 3. Worker Independence and Role Guidance

- **`independent_from`**: Enforces that the worker claiming this gate cannot share the same `worker_id` as the builder.
- **`fail`**: Selects the failure target; **`pass`** selects the success target. **`loop`** maps finding classes to failure targets.
- **`when`**: Selects a gate by scoped code, documentation, migration paths, concurrent seam mode, or AST public callable export.
- **`need`**: Names required review sources. **`save`** controls persistence, and **`absorb`** controls working-tree absorption for delivery.
- **`reuse_on_reject`**: If `true`, rejection guidance directs the task back to the original builder worker ID to remediate defects.
- **`mode`**: `subagent` or `inline` describe the intended role execution. Autonomous role dispatch is planned for AC-003.
- **`models`**: Ordered list of recommended LLM models and reasoning tiers for that specific role.
- The embedded `independent_reviewer` owns both review and delivery, so continue the same reviewer session through those gates. Its `NO_PREFLIGHT_SUBAGENTS` workflow rule prohibits launching preliminary subagents before the active gate work.

### 4. Boundary Context and Static Gates

Contract and build context exposes indexed `external_callers`, one-hop indexed
callers outside scope, plus `covering_tests`, which includes direct and two-hop
test dependencies. ADR entries include up to five invariant-summary lines.
`workflow_guidance.expected_evidence_template` carries the active claim identity
and proof shape. The admitted reachability contract requires the exact indexed test symbol to
reach a current, non-test production symbol inside scope within two supported
edges (`calls`, `imports`, `possible_calls`, `implements`, or `overrides`). The
check must fail closed when an AST index is unavailable. Round 4 (R4-18) found
that the audited implementation instead omitted the check without an index;
this enforcement gap remains under remediation. A stale or missing witness
returns `TASK_SEAM_DISCONNECTED` when the check runs. Refresh the claimed
worktree's code map after changing code used by the seam.
Contract and build reject changed public signatures with
out-of-scope indexed callers using `TASK_SCOPE_INCOMPLETE`. Rust methods in a
visible trait inherit that trait's visibility for signature-boundary checks,
whether the method has a body or only a declaration.

### 5. Anti-Tampering & SHA-256 Pinning

For auditability and protection against prompt injection or rogue agents altering workflow rules mid-execution:
- Pin custom profiles at planning time using the format `<path>:<sha256_prefix>`:
  ```yaml
  acdd_profile: ".forge/acdd/profile.yaml:a1b2c3d"
  ```
- During `task sync`, the file path and SHA-256 prefix are recorded in SQLite.
- On every `task claim` or `task submit`, Forge recalculates the disk hash. If altered, Forge aborts immediately with `TASK_PROFILE_TAMPERED`.
- Paths under `.forge/acdd/**` are protected; tasks cannot dynamically add them to `scope` during execution.

---

## Reference Examples

- See the canonical executable profile example in [`skills/contextunity-forge/references/acdd_profile.yaml.example`](../../skills/contextunity-forge/references/acdd_profile.yaml.example).
- See [Workspace configuration](configuration.md#declarative-acdd-profile-and-role-configuration) for full schema details and adapter settings.
