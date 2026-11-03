---
id: UX-002-lean-verification-and-forge-read
title: Autonomous command gates, lean verification diagnostics, profile output schemas, and unified layered forge_read
doc_type: contract
status: active
depends_on: []
guidance:
  hitl: pro
owners:
- src/cli/
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- crates/forge-core/src/
- crates/forge-index/src/db/
- crates/forge-tasks/src/engine/
- crates/forge-tasks/src/milestones/
- crates/forge-tasks/src/tasks/
- docs/reference/mcp-tools.md
- docs/reference/cli.md
- docs/reference/tasks.md
- docs/reference/mcp-markdown-contracts.md
- skills/contextunity-forge/
invariants:
- 'INV-AUTONOMOUS-COMMAND-GATE-WIRING: Zero-argument task_submit (across CLI and MCP) and task_verify automatically reuse execute_command_gate and compile_evidence_envelope, running the gate registered command in the claimed task worktree when no fresh claim-bound trace matches candidate_fingerprint, and expanding scope_args_by_pattern for task scope paths.'
- 'INV-PROFILE-DRIVEN-COMMAND-OUTPUT: The engine parses command stdout/stderr and validates gate proofs using declarative specifications in the active profile (commands.<id>.output with counts, require_counts, noise, errors, seam_ref_pattern), completely eliminating hardcoded Cargo string assumptions and enabling non-test linters, doc checkers, and multi-language toolchains without Rust binary modifications.'
- 'INV-WORKTREE-CONFINED-CONDITIONAL-TRACES: Command execution always runs inside the claimed task worktree (or repository root only for in-place milestone gates). Passing runs (exit 0 on build/verify, or matching expected_failure on contract) and short failures fitting the inline budget emit no external trace log files; full logs are persisted to <task_worktree>/.forge/task-traces/ and linked via file:/// URI only when diagnostic output is truncated.'
- 'INV-PRE-POST-WRITE-SCOPE-GUARD: Command verification checks git status --porcelain before and after process execution; any modified or untracked file outside the task admitted scope and extended_scope prepends a SCOPE_VIOLATION_WARNING without failing the process run itself.'
- 'INV-LAYERED-FORGE-READ: forge_read replaces get_code_snippet completely (removing get_code_snippet with zero aliases) and provides a layered capability model: core text reading (lines, raw, search, delta, output_file, null-byte binary check, and sensitive-pattern blocking from forge-mcp.yaml) is available in any workspace; structural symbols (snippet) and AST skeletons (outline) activate only when the codemap subsystem is enabled; doc section navigation activates when docs is enabled; task write-scope badges and gate verification hints activate when acdd is enabled.'
- 'INV-SIGNAL-DRIVEN-FILTERED-HINTS: Every forge_read mode appends a compact (1-3 line) conditional Hints footer containing pre-filled, copy-ready Forge tool calls (code_map_explain, code_map_impact, code_map_tests, get_doc, code_map_query implementations, task_manage verify, task_blackboard) strictly when the underlying SQLite graph edges or task state confirm the signal exists, filtered through filter_subsystem_tools so disabled subsystems never produce dead hints.'
required_skills:
- contextunity-forge
- agent-instructions
related_plans: []
started_at: 2026-11-03T08:59:21+00:00
---

# Autonomous command gates, lean verification diagnostics, profile output schemas, and unified layered `forge_read`

## Outcome and purpose

Deliver a streamlined, low-overhead developer loop and replace fragmented file-reading tools with an integrated, graph-aware, layered reader:

1. **Autonomous Command Gate Wiring (`execute_command_gate` $\leftrightarrow$ `submit_zero_arg` & `task verify`)**:
   - Connect `execute_command_gate` directly into `submit_zero_arg` so that zero-argument `task submit` (CLI) and `task_submit` without explicit `evidence` (MCP) automatically execute the gate's registered command in the claimed task worktree if no fresh completed trace matches `candidate_fingerprint`.
   - Expose explicit iterative verification via `task verify` (CLI) and `task_manage(action: "verify")` (MCP) to run and cache gate command checks without advancing SQLite task lifecycle states.
   - Run pre- and post-execution `git status --porcelain` against the task's admitted `scope` $\cup$ `extended_scope`, prepending `SCOPE_VIOLATION_WARNING` if out-of-scope files were created or modified.
   - Support hierarchical process timeouts: baseline timeout is configured per command in `profile.yaml` (`commands.<id>.timeout_seconds`, defaulting to 300s). Agents can supply a runtime override `timeout_seconds` (CLI `--timeout <secs>` or MCP argument `timeout_seconds`) up to `profile.limits.max_command_timeout_seconds` (default 1800s). When a process times out, the process group is terminated via `libc::kill(-pgid, SIGKILL)` and Forge emits a `TIMEOUT` diagnostic with an immediate rerun hint (e.g., `Hints: task_manage(action: "verify", timeout_seconds: 600)`).

2. **Declarative Profile Output Schemas (`commands.<id>.output`)**:
   - Eliminate hardcoded Rust/Cargo assumptions (`parse_test_counts` scanning for `test result:`, `validate_command` requiring `tests_passed > 0`, `seam_reference_from_command` expecting `--test <target>`).
   - Introduce `commands.<id>.output` in the profile specification:
     - `counts`: declarative regex patterns for extracting `passed` and `failed` counts across test frameworks (Cargo, pytest, Vitest, Jest, Go).
     - `require_counts`: boolean flag (defaults to `true` for test gates, `false` for linters, typecheckers, formatters, and documentation linters such as Vale). When `false`, `exit_code: 0` is accepted as full gate proof with zero test counts.
     - `noise`: regex patterns identifying progress noise to discard from diagnostic snippets (`^Compiling`, `^Downloading`, `^Finished`, `^✔`).
     - `errors`: regex patterns identifying failure and error blocks to extract for inline diagnostic summaries (`^error(\[E\d+\])?:`, `panicked at`, `FAIL `).
     - `seam_ref_pattern`: regex pattern extracting seam test identifiers from command strings without assuming Rust `--test` syntax.

3. **Worktree-Confined Trace Files & Adaptive Output Budgets**:
   - Direct all gate command execution and trace logs into the **claimed task worktree** (`<task_worktree>/.forge/task-traces/...`). Task worktree destruction cleanly prunes raw execution logs without repository bloat.
   - Enforce adaptive output thresholds:
     - **Passing run**: return a compact 1–2 line confirmation (`PASS: <command> — exit 0, N passed, 0 failed (X.Xs)`) and store the snippet in SQLite `gate_execution_traces.output_snippet` without generating or linking unnecessary `.log` files.
     - **Short failure** (cleaned diagnostic output $\le \text{profile.limits.diagnostic_inline_bytes}$, default 4 KiB): display compiler/test errors directly inline without creating external log files.
     - **Truncated large failure**: persist the raw log to `<task_worktree>/.forge/task-traces/<task_key>/<gate_key>/<attempt>-<cmd>.log` and return an inline excerpt (head 30% + tail 20%) accompanied by a clickable `file:///` link.

4. **Layered Graph-Aware Reader (`forge_read`, replacing `get_code_snippet`)**:
   - Register `forge_read` (in CLI: `contextunity-forge-mcp read` and MCP: `forge_read`) and **completely remove `get_code_snippet` without aliases**.
   - Update all generated next-tool hints across the entire codebase (`code_map_*`, `search_docs`, `forge_guide`, skills) to reference `forge_read`.
   - The reader functions across four distinct subsystem layers:

| Layer (`Шар`) | Activation (`Коли діє`) | Capabilities & Hints (`Що додає`) |
| :--- | :--- | :--- |
| **`core`** | Always active (any workspace, even `minimal` or non-code) | Windowed text reads (`lines`, `raw`), substring/regex `search` with context, Git unified `delta` (against `HEAD` or task `candidate_baseline_head`), `output_file` export, first 8 KiB null-byte binary check (`BINARY_FILE`), and sensitive file blocking from `forge-mcp.yaml` (`sensitive: [...]`). |
| **`docs`** | When `docs` subsystem is enabled | Hierarchical heading outline for Markdown (`#`, `##`, `###`), and contextual next-step hints `get_doc(path: "<doc>#<section>")` and `search_docs`. |
| **`codemap`** | When `codemap` subsystem is enabled | Symbol-bound source extraction (`snippet` by selector `path:Symbol`), Tree-sitter structural skeletons (`outline`), graph pulse annotations (counts for callers, covering tests, invariants), and next-step hints for `code_map_explain`, `code_map_impact`, `code_map_tests`, and `code_map_query`. |
| **`acdd`** | When `acdd` subsystem is enabled | In-scope write status badges (`ALLOWED` vs `OUT_OF_SCOPE`), and task lifecycle hints: `task_manage(action: "extend_scope")`, `task_blackboard`, and `task_manage(action: "verify")`. |

   - All generated `Hints:` footers are strictly filtered through `filter_subsystem_tools` to prevent offering disabled tools.

---

## Tasks in this milestone

### task: contracts-and-shared-types-bootstrap

```yaml
task_ref: contracts-and-shared-types-bootstrap
target: Define shared command output schemas, verification data types, layered read models, error variants, and pre-register seam test stubs for the milestone
proof_policy: seam-test-first
contract_revision: 2
depends_on: []
scope:
- crates/forge-tasks/src/tasks/profile.rs
- crates/forge-core/src/subsystems.rs
- crates/forge-core/src/model.rs
- tests/acdd/runner.rs
- tests/mcp/test_read.rs
- tests/acdd/tasks/lifecycle.rs
- crates/forge-core/src/lib.rs
- crates/forge-tasks/src/verification.rs
- tests/mcp.rs
- docs/reference/profiles.md
symbols:
- struct:CommandOutputSpec
- struct:CommandCountsSpec
- enum:ReadMode
- struct:ReadBounds
- struct:ReadResult
- struct:VerifyOutcome
- enum:VerifyDiagnosticMode
status: completed
subtasks:
- subtask_ref: command-output-spec-schema
  title: Define CommandOutputSpec and CommandCountsSpec in profile.rs with serde support for counts, require_counts, noise, errors, and seam_ref_pattern
  status: completed
  evidence: 'cargo test --test acdd: 146 passed, 0 failed; profile output, legacy formats, defaults and invalid regex tests passed; strict Clippy passed'
- subtask_ref: verification-types-and-diagnostics
  title: Define VerifyOutcome, VerifyDiagnosticMode, and scope violation alert structures in forge-tasks
  status: completed
  evidence: 'cargo test --test acdd: 146 passed, 0 failed; strict cargo clippy --all-targets --all-features -- -D warnings passed'
- subtask_ref: layered-read-model-and-subsystems
  title: Define ReadMode, ReadBounds, and ReadResult in forge-core decoupled from codemap dependency and register forge_read in SubsystemRegistry
  status: completed
  evidence: 'cargo test --test mcp: 93 passed, 0 failed; reader model and minimal/docs/codemap subsystem tests passed; strict Clippy passed'
- subtask_ref: seam-test-stubs-pre-registration
  title: Pre-register minimal compiling test stubs in tests/acdd/runner.rs and tests/mcp/test_read.rs
  status: completed
  evidence: 'cargo test --test acdd: 146 passed, 0 failed; cargo test --test mcp: 93 passed, 0 failed; both test modules compile and run'
extended_scope:
- tests/acdd/tasks/lifecycle.rs
- crates/forge-core/src/lib.rs
receipt:
  contract_revision: 2
  passed_at: 2026-11-03T09:31:20.350863153+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/acdd/runner.rs::command_profile_preserves_declarative_output_specification
      tests_passed: 146
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed scoped snapshot 5a9bde8cfd9e5f779361a4df702243b83b337400 against baseline 304a425b358ba651d53a566f6caded15e23a0ee0 and current Git status, including staged-deletion/untracked replacements. All ten scoped file blob IDs match the snapshot. Each change belongs to the bootstrap type/schema/test/documentation slice; protected Cargo.toml, build.rs, profile/configuration and credential paths are unchanged. The milestone manifest diff records prior activation and admitted contract revision 2, not a source scope escape. git diff --check exited 0.
        claims:
          applicable: true
          evidence: 'Acceptance 1-2: CommandOutputSpec/CommandCountsSpec deserialize optional counts, noise/errors/seam regexes and require_counts=true; Detailed and Pattern commands expose the same optional spec while legacy strings/mappings serialize without a new output field. Both profile versions call the shared regex validator. The contract seam directly calls production parse_profile (confirmed exact graph edge); dedicated tests cover preservation, legacy/defaults, and invalid regex rejection through validate_profile_document. Acceptance 3-5: public read DTOs use serde in the unchanged forge-core leaf, with no forge-index/tree-sitter dependency; canonical SubsystemRegistry keeps forge_read universal and mode gating distinguishes Markdown docs outline from codemap source outline/snippet. tests/mcp/test_read.rs is registered and exercises these seams. VerifyOutcome/VerifyDiagnosticMode/ScopeViolationAlert live at the existing verification-domain owner. docs/reference/profiles.md accurately documents the admitted schema; downstream execution/read behavior and assigned example co-evolution remain outside this bootstrap slice. Performance review found no indexing/storage path changes, duplicate registries/caches, nested parallel work or repeated source I/O. Independent review verification on the unchanged candidate: cargo clippy --all-targets --all-features -- -D warnings exited 0 (no test count); cargo test --test acdd exited 0 with 146 passed, 0 failed, 0 ignored, 0 filtered; cargo test --test mcp exited 0 with 93 passed, 0 failed, 0 ignored, 0 filtered. Combined command exit 0, 239 tests passed. Raw verification archive: b132ff53bc8cb85b. Post-check hashing reconfirmed all ten scoped blobs match candidate 5a9bde8cfd9e5f779361a4df702243b83b337400.'
        concurrency:
          applicable: false
          evidence: The candidate adds immutable data models, serde schema/defaults, profile validation and pure subsystem gating only. It changes no transactions, claim fences, revision checks, locks, receipt persistence, candidate baselines or filesystem/Git mutation flow; existing lifecycle/concurrency behavior is preserved and covered by the full ACDD target.
        project_isolation:
          applicable: true
          evidence: Reviewed canonical forge-core subsystem registry and forge-tasks catalog re-export, model module ownership and profile validation path. New DTOs and mode gating perform no repository traversal, file reads/writes, workspace resolution, database access or subprocess execution, so they introduce no cross-project access path. Existing roots, symlink guards, task IDs, Git scopes and profile source selection are unchanged.
        administration:
          applicable: true
          evidence: No storage/schema migration, privileged delivery change, hook handling, profile pin/hash implementation, receipt rewrite, Cargo manifest/build script or repository profile change is present. Optional output is omitted when absent, retaining existing command serialization for legacy profiles. Invalid configured regexes fail at the canonical profile validator. Candidate was kept unchanged throughout independent review; all prior accepted authority and delivery mechanisms remain intact.
      extended_scope_review:
        approved_paths:
        - tests/acdd/tasks/lifecycle.rs
        - crates/forge-core/src/lib.rs
        reason_verified: true
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-AUTONOMOUS-COMMAND-GATE-WIRING: Zero-argument task_submit (across CLI and MCP) and task_verify automatically reuse execute_command_gate and compile_evidence_envelope, running the gate registered command in the claimed task worktree when no fresh claim-bound trace matches candidate_fingerprint, and expanding scope_args_by_pattern for task scope paths.'
    - 'INV-PROFILE-DRIVEN-COMMAND-OUTPUT: The engine parses command stdout/stderr and validates gate proofs using declarative specifications in the active profile (commands.<id>.output with counts, require_counts, noise, errors, seam_ref_pattern), completely eliminating hardcoded Cargo string assumptions and enabling non-test linters, doc checkers, and multi-language toolchains without Rust binary modifications.'
    - 'INV-WORKTREE-CONFINED-CONDITIONAL-TRACES: Command execution always runs inside the claimed task worktree (or repository root only for in-place milestone gates). Passing runs (exit 0 on build/verify, or matching expected_failure on contract) and short failures fitting the inline budget emit no external trace log files; full logs are persisted to <task_worktree>/.forge/task-traces/ and linked via file:/// URI only when diagnostic output is truncated.'
    - 'INV-PRE-POST-WRITE-SCOPE-GUARD: Command verification checks git status --porcelain before and after process execution; any modified or untracked file outside the task admitted scope and extended_scope prepends a SCOPE_VIOLATION_WARNING without failing the process run itself.'
    - 'INV-LAYERED-FORGE-READ: forge_read replaces get_code_snippet completely (removing get_code_snippet with zero aliases) and provides a layered capability model: core text reading (lines, raw, search, delta, output_file, null-byte binary check, and sensitive-pattern blocking from forge-mcp.yaml) is available in any workspace; structural symbols (snippet) and AST skeletons (outline) activate only when the codemap subsystem is enabled; doc section navigation activates when docs is enabled; task write-scope badges and gate verification hints activate when acdd is enabled.'
    - 'INV-SIGNAL-DRIVEN-FILTERED-HINTS: Every forge_read mode appends a compact (1-3 line) conditional Hints footer containing pre-filled, copy-ready Forge tool calls (code_map_explain, code_map_impact, code_map_tests, get_doc, code_map_query implementations, task_manage verify, task_blackboard) strictly when the underlying SQLite graph edges or task state confirm the signal exists, filtered through filter_subsystem_tools so disabled subsystems never produce dead hints.'
    architectural_notes:
    - 'decisions: Architectural synthesis: Extend the existing CommandDef detailed profile schema with typed optional output and counts; preserve legacy script and detailed forms, validate patterns at profile boundary, and have command execution/proof compilation consume that same profile definition. Existing command_gate is the process/trace owner and submit::compile_evidence_envelope is the claim/fingerprint proof owner; avoid duplicate registries. Read types belong in the forge-core leaf, universal reader routing uses existing SubsystemRegistry and filter_subsystem_tools, indexed snippet extraction remains owned by forge-index; disabled structural modes must fail closed. Execution must bind claimed worktree, preserve candidate and trace integrity, and distinguish expected-red results from failures. Graph inspection found an additional CommandDef initializer in tests/acdd/tasks/lifecycle.rs; extended scope for compilation. tests/mcp/test_read.rs needs registration in tests/mcp.rs, but extend_scope rejected that path outside admitted roots; resolve authority before adding that registration. Protected Cargo.toml, build.rs and .forge files remain untouched.'
    - 'decisions: Revision 2 synthesis: milestone-wide data flow and scope findings are recorded in milestone #1538 and decision #1539. Bootstrap must use existing profile and verification owners, register core read model in lib.rs and the test module in tests/mcp.rs, and update profile documentation; tests/acdd/tasks/lifecycle.rs is included because adding CommandDef::Detailed.output changes the constructor. Red profile parser seam remains current (exit 101). Preserve serde compatibility and leaf dependency boundaries; no protected files.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `CommandOutputSpec` deserializes cleanly from YAML with optional `counts`, `require_counts` (defaulting to `true`), `noise` list, `errors` list, and `seam_ref_pattern`.
2. `CommandDef::Detailed` incorporates `output: Option<CommandOutputSpec>` without breaking backward compatibility for string-only scripts or legacy detailed commands.
3. `ReadMode` (`Auto`, `Snippet`, `Outline`, `Lines`, `Raw`, `Delta`) and `ReadResult` are defined in `forge-core` without mandatory dependencies on `tree-sitter` or `forge-index`.
4. `subsystems.rs` registers `forge_read` as an eager/universal tool under the `core` layer and gates `snippet`/`outline` operations on `codemap`.
5. Seam test harness stubs compile cleanly in `tests/acdd/runner.rs` and `tests/mcp/test_read.rs`.

---

### task: autonomous-command-execution-and-trace-diagnostics

```yaml
task_ref: autonomous-command-execution-and-trace-diagnostics
target: "Wire execute_command_gate into zero-arg task_submit and task verify, apply profile CommandOutputSpec parsing, enforce task-worktree confinement, and guard write scopes"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - contracts-and-shared-types-bootstrap
scope:
  - crates/forge-tasks/src/milestones/command_gate.rs
  - crates/forge-tasks/src/engine/submit.rs
  - crates/forge-tasks/src/engine/lifecycle.rs
  - crates/forge-tasks/src/tasks/gates.rs
  - src/cli/task.rs
  - docs/reference/tasks.md
  - docs/reference/cli.md
  - tests/acdd/runner.rs
symbols:
  - fn:submit_zero_arg
  - fn:execute_verify_check
  - fn:parse_command_output
  - fn:check_write_scope_violations
status: planned
subtasks:
  - subtask_ref: zero-arg-submit-auto-wiring
    title: "Invoke execute_command_gate inside submit_zero_arg when no fresh completed run matches candidate_fingerprint"
  - subtask_ref: profile-output-spec-parser
    title: "Implement parse_command_output using profile CommandOutputSpec regexes for counts and error extraction, replacing hardcoded Cargo strings"
  - subtask_ref: require-counts-gate-validation
    title: "Update validate_command to permit tests_passed == 0 when require_counts is false and exit_code is 0"
  - subtask_ref: worktree-confinement-and-trace-thresholds
    title: "Confine traces to <task_worktree>/.forge/task-traces/, emit 1-line pass summaries without files, display short failures inline, and link large failures"
  - subtask_ref: pre-post-scope-guard-and-task-verify-cli
    title: "Perform pre/post git status checks for SCOPE_VIOLATION_WARNING, support runtime timeout_seconds overrides in task verify/submit (CLI/MCP), and emit rerun hints on process timeout"
```

#### Acceptance

1. Calling `task submit` (CLI without `--evidence`) or `task_submit` (MCP without `evidence`) on a `contract` or `build` gate automatically executes the registered command in the claimed task worktree if no fresh run exists for the current `candidate_fingerprint`.
2. When `task verify` (CLI) or `task_manage(action: "verify")` is invoked, the gate command runs and writes an execution trace without advancing the task's stage in SQLite. A subsequent `task submit` with identical working tree state reuses this cached execution immediately.
3. Test frameworks and non-test tools configure their output parsing via `commands.<id>.output`:
   - Non-test commands (linters, typecheckers, doc linters) declaring `require_counts: false` pass validation with `exit_code: 0` and zero test counts.
   - Test commands parse `tests_passed` and `tests_failed` via declarative regex patterns without relying on Rust Cargo strings.
   - Seam test references are extracted via `seam_ref_pattern` without assuming `--test <target>`.
4. Output log files are stored exclusively under `<task_worktree>/.forge/task-traces/`:
   - Passing runs emit a 1–2 line summary without creating or referencing log files.
   - Short failures fitting `profile.limits.diagnostic_inline_bytes` (default 4 KiB) print inline error blocks without creating log files.
   - Large truncated failures persist the full raw log and return the inline excerpt plus a clickable `file:///` link.
5. If untracked or modified files outside the task's admitted scope exist before or after execution, a `SCOPE_VIOLATION_WARNING` header is prepended to the command output.
6. Commands inherit `timeout_seconds` from `profile.commands.<id>` (defaulting to 300s) and accept a runtime override via CLI `--timeout <secs>` or MCP `timeout_seconds`. If a command exceeds this limit, the process group is terminated cleanly via `libc::kill(-pgid, SIGKILL)`, a `TIMEOUT` diagnosis is returned with the elapsed duration, and a hint suggests rerunning with an increased timeout (e.g. `Hints: task_manage(action: "verify", timeout_seconds: 600)`) bounded by `profile.limits.max_command_timeout_seconds`.

---

### task: unified-forge-read-and-subsystem-slices

```yaml
task_ref: unified-forge-read-and-subsystem-slices
target: "Implement unified layered forge_read replacing get_code_snippet across core, docs, codemap, and acdd layers, and update all tool hints"
proof_policy: seam-test-first
contract_revision: 1
depends_on:
  - contracts-and-shared-types-bootstrap
scope:
  - src/mcp/tools.rs
  - src/mcp/tools/inputs.rs
  - src/cli/mod.rs
  - crates/forge-index/src/docs.rs
  - crates/forge-tasks/src/engine/context.rs
  - docs/reference/mcp-tools.md
  - docs/reference/mcp-markdown-contracts.md
  - skills/contextunity-forge/
  - tests/mcp/test_read.rs
symbols:
  - fn:forge_read_tool
  - fn:render_read_hints
  - fn:extract_file_slice
  - fn:compute_file_delta
status: planned
subtasks:
  - subtask_ref: forge-read-tool-registration
    title: "Register forge_read tool (CLI: contextunity-forge-mcp read) and completely remove get_code_snippet without alias"
  - subtask_ref: core-layer-windowed-and-delta-reads
    title: "Implement core layer: lines, raw, and search modes with canonical path confinement, binary null-byte check, sensitive pattern blocking from forge-mcp.yaml, and git delta diff"
  - subtask_ref: docs-layer-markdown-outline
    title: "Implement docs layer: outline mode parsing Markdown headings (#, ##, ###) with get_doc and search_docs hints when docs subsystem is active"
  - subtask_ref: codemap-layer-snippets-and-ast-skeletons
    title: "Implement codemap layer: snippet by symbol selector and outline via Tree-sitter with caller/test/invariant counts when codemap subsystem is active"
  - subtask_ref: acdd-layer-scope-badges-and-hints
    title: "Implement acdd layer: write_scope status badges (ALLOWED vs OUT_OF_SCOPE), task_manage extend_scope, blackboard, and verify hints"
  - subtask_ref: purge-get-code-snippet-and-update-hints
    title: "Replace all legacy get_code_snippet references in code_map_*, search_docs, forge_guide, and skill prompt templates with forge_read"
```

#### Acceptance

1. `forge_read` is registered as a first-class MCP tool and CLI command (`contextunity-forge-mcp read`). `get_code_snippet` is completely purged from MCP tool registrations (no lingering aliases).
2. The layer model is strictly observed:
   - Core operations (`lines`, `raw`, `search`, `delta`, `output_file`) work in any workspace regardless of enabled subsystems.
   - Calling `mode: "outline"` on a `.md` file generates a heading tree when `docs` is active.
   - Calling `mode: "snippet"` or `mode: "outline"` on a source file requires `codemap`, annotating output with caller/test/invariant counts.
   - When `acdd` is active, files in the claimed task's write scope receive `[write_scope: ALLOWED]`, while other files receive `[write_scope: OUT_OF_SCOPE]`.
3. Path confinement prevents directory traversal outside configured workspace and linked roots; sensitive files matching `forge-mcp.yaml` `sensitive:` patterns are rejected; binary files are safely reported as `BINARY_FILE`.
4. In `delta` mode, the tool returns unified diff hunks against `HEAD` or the active task's `candidate_baseline_head` stored in `.forge/tasks.sqlite`.
5. All tool hints across Forge (`code_map_inspect`, `code_map_explain`, `search_docs`, etc.) emit copy-ready `forge_read(...)` calls instead of `get_code_snippet(...)`.
6. Every `Hints:` footer emitted by `forge_read` is passed through `filter_subsystem_tools` to ensure disabled subsystems never produce dead hints.
