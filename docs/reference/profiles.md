---
title: "ACDD Profiles Reference"
doc_type: api
---

# ACDD Profiles Reference

An **ACDD Profile** (`profile.yaml`) defines the governance model for a task or milestone: its ordered verification gates, proof evaluation policies, review contours, worker role assignments, and recommended AI models.

By separating workflow governance from code and task specifications, profiles allow teams to enforce strict multi-agent compliance (e.g. mandatory independent reviewer separation, TDD seam-first gates, and SHA-256 anti-tampering verification).

The embedded profile uses three subagent roles: `contract_author` (`sol-6.1:high`), `builder` (`gpt6-luna:xhigh`), and `independent_reviewer` (`sol-6.1:xhigh`). The contract author targets a draft to `build` on the task blackboard. The independent reviewer keeps the same worker session through `review` and `deliver`, remaining distinct from the builder. Claim guidance includes `NO_PREFLIGHT_SUBAGENTS` so gates start without preflight agents.

---

## Profile Loading Precedence

Forge resolves the active profile in the following order:

1. **Task specification**: Pinned in task metadata as `acdd_profile: "<path>:<sha256_prefix>"`.
2. **Milestone frontmatter**: Declared in milestone YAML frontmatter as `acdd_profile: "<path>:<sha256_prefix>"`.
3. **Workspace adapter**: Referenced in `forge-mcp.yaml` under `acdd_profile: "<path>"`.
4. **Repository default**: File present at `.forge/acdd/profile.yaml`.
5. **Embedded fallback**: Built-in default workflow (`acdd.default.yaml`).

> [!NOTE]
> Linked workspaces (`linked_workspaces`) do not override the parent ACDD profile. Only the active workspace root governs the task profile.

---

## Profile Structure

A complete profile YAML consists of five primary sections:

```yaml
# .forge/acdd/profile.yaml
gates:
  - id: contract
    role: contract_author
    proof: contract
    steps:
      - "Author a sensitive failing seam test."
  - id: build
    role: builder
    proof: command
    sha_snapshot: true
    steps:
      - "Implement approved contract inside the declared allowed_write_scope."
  - id: review
    role: independent_reviewer
    proof: review
    independent_from: build
    reject_to: build
    contours: standard
    steps:
      - "Inspect candidate diff and verify contours."
  - id: deliver
    role: independent_reviewer
    proof: delivery
    auto_commit: true
    independent_from: build
    review_sources:
      - review
    reject_to: build
    contours: standard
    steps:
      - "Submit delivery proof."

contours:
  standard:
    paths:
      description: "Verify changes stay within the task's admitted write perimeter."
      criteria:
        - "Compare every changed file with the task's initial scope."
    claims:
      description: "Verify each accepted contract claim is implemented."
      criteria:
        - "Trace every invariant to the production code path."

roles:
  contract_author:
    mode: subagent
    reuse_on_reject: true
    models:
      - 'sol-6.1:high'
    guidance: "Define the production-seam contract."

  builder:
    mode: subagent
    reuse_on_reject: true
    models:
      - 'gpt6-luna:xhigh'
    guidance: "Implement passing production code and green unit tests."

  independent_reviewer:
    mode: subagent
    reuse_on_reject: true
    models:
      - 'sol-6.1:xhigh'
    guidance: "Audit the candidate diff across the quality contours."

commands:
  rust-check-and-test:
    command: "cargo clippy --all-targets --all-features -- -D warnings && cargo test"
    description: "Run strict Rust checks followed by the repository's test suite."
  test:
    command: "cargo test"
    description: "Run test suite."

policies:
  delivery:
    atomic_receipt: true
  proof:
    command:
      require_zero_exit: true
  review:
    independent_worker: true
    require_all_contours: true
```

The embedded profile also registers `test: cargo test` for compatibility with
legacy task evidence. Workflow guidance selects `rust-check-and-test`, the
canonical strict Clippy-plus-test command. A scoped command appends the
supported `--test <target>` suffix to its final `cargo test`. Claim context derives
`workflow_guidance.scoped_commands` from the indexed contract seam and covering
tests, and provides `expected_evidence_template` bound to the active task,
stage, revisions, worker, and worktree.

---

## Key Concepts

### 1. Gates and Proof Kinds

Each gate declares a stable identifier (`id`) and a proof specification (`proof`):
- `contract`: Verifies a red seam test (default `seam-test-first` expecting non-zero exit code) or invariant baseline (`direct-proof`).
- `command`: Executes and verifies a registered CLI command (must exit with code 0).
- `review`: Evaluates 5 review contours and enforces worker separation (`independent_from`).
- `delivery`: Terminal gate that writes the durable receipt into SQLite and milestone Markdown.
- `scheme`: A custom recursive JSON schema validation under `proof: { scheme: ... }`.

### 2. Worker Independence and Role Guidance

- **`independent_from`**: Enforces that the worker claiming this gate cannot share the same `worker_id` as the builder.
- **`reject_to`**: Specifies which prior gate to rewind to if review fails.
- **`reuse_on_reject`**: If `true`, rejection guidance directs the task back to the original builder worker ID to remediate defects.
- **`mode`**: `subagent` (launches an isolated agent context) or `inline` (runs within the parent agent).
- **`models`**: Ordered list of recommended LLM models and reasoning tiers for that specific role.
- The embedded `independent_reviewer` owns both review and delivery, so continue the same reviewer session through those gates. Its `NO_PREFLIGHT_SUBAGENTS` workflow rule prohibits launching preliminary subagents before the active gate work.

### 3. Boundary Context and Static Gates

Contract and build context exposes indexed `external_callers`, one-hop indexed
callers outside scope, plus `covering_tests`, which includes direct and two-hop
test dependencies. ADR entries include up to five invariant-summary lines.
`workflow_guidance.expected_evidence_template` carries the active claim identity
and proof shape. Contract submission requires the exact indexed test symbol to
reach a current, non-test production symbol inside scope within two supported
edges (`calls`, `imports`, `possible_calls`, `implements`, or `overrides`). It
checks each hop's source and target identity, current typed outgoing references,
and applicable import bindings against source parsed from the claimed worktree,
using the claimed worktree's local index when present and the shared read-only
index otherwise. A stale or missing witness returns `TASK_SEAM_DISCONNECTED`;
refresh the claimed-worktree code map after changing code used by the seam.
Contract and build reject changed public signatures with
out-of-scope indexed callers using `TASK_SCOPE_INCOMPLETE`. Rust methods in a
visible trait inherit that trait's visibility for signature-boundary checks,
whether the method has a body or only a declaration.

### 4. Anti-Tampering & SHA-256 Pinning

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
