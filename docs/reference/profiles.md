---
title: "ACDD Profiles Reference"
doc_type: api
---

# ACDD Profiles Reference

An **ACDD Profile** (`profile.yaml`) defines the governance model for a task or milestone: its ordered verification gates, proof evaluation policies, review contours, worker role assignments, and recommended AI models.

By separating workflow governance from code and task specifications, profiles allow teams to enforce strict multi-agent compliance (e.g. mandatory independent reviewer separation, TDD seam-first gates, and SHA-256 anti-tampering verification).

The embedded profile uses three subagent roles: `contract_author` (`sol-6.1:high`), `builder` (`gpt6-luna:xhigh`), and `independent_reviewer` (`sol-6.1:xhigh`). The contract author targets a draft to `build` on the task blackboard. The independent reviewer keeps the same worker session through `review` and `deliver`, remaining distinct from the builder. Claim guidance includes `NO_PREFLIGHT_SUBAGENTS` so gates start without preflight agents.

> [!NOTE]
> Contract, command, and seam-test passes require claim-bound execution observations. Seam-test references require individual passed-test observations, not only counts; see [supported commands](tasks.md). Initial milestone handoff requires a passing root command transition and latest matching execution trace bound to final content and target base. Validated archived and journal retries follow their [recovery rules](tasks.md#milestone-cli-lifecycle). These checks do not implement the deferred lifecycle-kernel or immutable historical-source redesigns. Native headless dispatch is described in [CLI commands](cli.md#headless-task-execution).

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

## Profile Structure and Inheritance (`inherit: default` vs `inherit: none`)

Profiles inherit from the embedded baseline (`acdd.default.yaml`) by default (`inherit: default` or omitted `inherit`). Under `inherit: default`:
- `roles`, `proofs`, `commands`, `guidance`, `policies`, and `scope_args_by_pattern` merge by key.
- `gates:` merges **recursively by gate `id` (and `- tasks:` container)** before hierarchy normalization: matching child gates update only the specified fields (such as binding `command: rust-check-and-test`), while preserving inherited roles, guidance, policies, contours, `save` hooks, and `proof` definitions.

Set `inherit: none` to compile a standalone profile (such as `author-milestone.workflow.yaml`, `author-plan.workflow.yaml`, or `author-workflow.workflow.yaml`) strictly from its own declarations without inheriting base roles, gates, commands, policies, proofs, or guidance entries. An explicit `commands: {}` clears an inherited command catalog; omit `commands` to keep the inherited catalog. A profile with an empty catalog must not declare gates that require command proof.

Because `inherit: default` deep-merges `gates:` by `id`, a repository `.forge/acdd/profile.yaml` overlay only needs to declare project-specific commands, `scope_args_by_pattern`, and gate command bindings:

```yaml
# .forge/acdd/profile.yaml (concise repository overlay over acdd.default.yaml)
version: 2
inherit: default

scope_args_by_pattern:
  "src/lib.rs": "--lib"
  "src/main.rs": "--bin contextunity-forge-mcp"
  "tests/*.rs": "--test {file_stem}"
  "tests/**/*.rs": "--test acdd"

commands:
  rust-check-and-test:
    by_pattern:
      "**/*.rs": "cargo clippy --all-targets --all-features -- -D warnings && cargo test"
    description: "Run strict Rust checks followed by the repository's test suite."
  check_full_repo:
    by_pattern:
      "**/*.rs": "cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets"
    description: "Run milestone handoff verification commands from TESTS.md."

gates:
  - tasks:
      gates:
        - id: build
          command: rust-check-and-test
        - id: seam
          when: seam
          gates:
            - id: integrate
              command: rust-check-and-test
  - id: verify
    command: check_full_repo
```

The full canonical gate hierarchy resolved from [`acdd.default.yaml`](../../crates/forge-tasks/src/tasks/acdd.default.yaml) (`blueprint_synthesis` $\to$ `tasks:` $\to$ `verify` / `polish` / `milestone_review` / `handoff`) is documented gate-by-gate in [Built-in Default Profile Walkthrough](acdd.md#built-in-default-profile-walkthrough-acdddefaultyaml). See [`acdd_profile.yaml.example`](acdd_profile.yaml.example) (symlinked from the Forge skill) for a complete standalone `inherit: none` reference template.

Workflow guidance selects `rust-check-and-test`, the canonical strict Clippy-plus-test command. Profiles may define
`scope_args_by_pattern` as project-relative path patterns mapped to command
arguments. The most specific matching pattern wins across profile and command
patterns; command-local patterns win ties. `{file_stem}` expands
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
`errors` are lists of regular expressions. (Seam test identities are supplied directly
via `verify --test <test_ref>` rather than parsed via regex heuristics). The optional `tests` mapping defines declarative rules for
extracting individual test outcomes and source identities:
`passed` (required pattern matching an individual passed test and capturing its name/ref in group 1),
`failed` (optional pattern matching an individual failed test in group 1),
`ignored` (optional pattern matching an individual skipped/ignored test in group 1), and
`target` (optional pattern extracting the test target/entrypoint source file in group 1).
Count and individual test patterns extract capture group 1; the profile
validator rejects invalid expressions and extraction patterns without that group.
Counts are summed across matching summaries. When a configured passed count is
present and the failed pattern has no match, the failed count is zero. Missing
required counts, malformed integers, overflow, and reported failures reject proof.
Set `require_counts: false` for linters and documentation checks whose zero exit
code proves success without a test count. Configure output rules to describe command counts and test identities.

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

`limits.diagnostic_inline_bytes` defaults to 4096 and accepts 1..=98304 bytes.
Passing commands and matching contract failures produce compact summaries without
logs. Short failures remain inline; larger cleaned diagnostics retain head and
tail excerpts and link the raw log under the claimed worktree's
`.forge/task-traces/`. `limits.max_command_timeout_seconds` defaults to 1800 and
bounds positive runtime timeout overrides, including requests that reuse cached
proof. Each command's `timeout_seconds` supplies its ordinary timeout (300 seconds
when omitted). See [task verification](tasks.md) for CLI and MCP operations.

---

## Key Concepts

### 1. Gates, Automatic Proof Inference, and `proofs:` Catalog / Inline Schemas

Each gate declares a stable identifier (`id`). The `proof` field is optional because [`infer_gate_proof`](../../crates/forge-tasks/src/tasks/profile/normalize.rs) automatically infers the internal verification mode (`ProofKind`) from the gate's structural attributes during profile normalization in this priority order:
1. `receipt: true` $\implies$ `delivery`: Terminal gate that writes the durable receipt into SQLite and milestone Markdown and creates the delivery commit (`save.post: commit`).
2. `expected_failure` present $\implies$ `contract`: Verifies a runtime-red seam test matching `expected_failure.exit_codes` (or invariant baseline when `proof_policy` is `direct-proof`).
3. `command` present $\implies$ `command`: Executes and verifies a registered CLI command (must exit with code `0` and satisfy configured `output` count rules).
4. `policy` or `contours` present $\implies$ `review`: Evaluates cognitive review contours (`contours: guidance.contours`) and enforces worker separation (`independent_from`).
5. Otherwise $\implies$ `none`: Informational step or container gate.

An explicit `proof` declaration is used when overriding the inferred behavior, referencing a reusable schema in the top-level `proofs:` catalog, or attaching an inline JSON Schema directly to the gate:
- `proof: contract`: Explicitly marks a contract gate when it relies on task `proof_policy` rather than inline `expected_failure`.
- `proof: seam_test`: Used on `seam.integrate` (`when: seam`) to verify element-by-element that every test identifier in `seam_test_refs` executed and passed (`exit_code == 0`) in the captured trace.
- `proof: <catalog_name>` (e.g. `proof: blueprint`): References a reusable schema declared under top-level `proofs.<catalog_name>.schema` (`ProofCatalogDef`), allowing multiple gates and inherited profiles to share the same schema definition.
- `proof: { scheme: { ... } }`: Declares an inline recursive JSON Schema directly inside a single gate.

When any pre-task root gate (`cursor.root_index < task_position`, such as `blueprint_synthesis.synthesize`) passes with a `proof` schema payload (`proof: blueprint` or `proof: { scheme: ... }`) containing a per-task dictionary `tasks: { "<task_ref>": { ... } }`, the engine automatically records a `gate_transitions` row per task (`subject = <full_task_id>`) and injects strictly that task's slice into `context_bundle` upon `task claim`.

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

- **`independent_from`**: At claim and submission, requires a `worker_id` different from the accepted worker of the named predecessor gate.
- **`fail`**: Selects the failure target; **`pass`** selects the success target. **`loop`** maps finding classes to failure targets.
- **`when`**: Selects a gate by scoped code, documentation, migration paths, concurrent seam mode, or AST public callable export.
- **`need`**: Names required review sources. **`save`** controls persistence, and **`absorb`** controls working-tree absorption for delivery.
- **`reuse_on_reject`**: If `true`, rejection guidance directs the task back to the original builder worker ID to remediate defects.
- **`mode`**: `subagent` or `inline` describe the intended role execution. Headless commands execute configured role runners as described in the [CLI reference](cli.md#headless-task-execution).
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

- See the canonical executable profile template in [`acdd_profile.yaml.example`](acdd_profile.yaml.example) (symlink to the full specification in `skills/contextunity-forge/`).
- See [Workspace configuration](configuration.md#declarative-acdd-profile-and-role-configuration) for full schema details and adapter settings.
