---
id: m-markdown-first-dual-surface-and-agent-ergonomics
title: Markdown-First Dual-Surface Architecture, Rich Code Intelligence, and Agent Ergonomics
doc_type: contract
status: active
depends_on:
- m-documentation-test-graph-and-tool-ergonomics
owners:
- src/core/format/
- src/core/mod.rs
- src/core/response.rs
- src/mcp/
- src/cli/
- src/db/reader.rs
- src/db/symbols.rs
- src/db/symbols/
- src/db/traversal.rs
- src/engine/scanner/
- src/engine/ast/
- src/engine/languages/
- src/engine/tasks/
- docs/adr/0019-mcp-markdown-output-and-dead-metadata-compaction.md
- docs/reference/mcp-markdown-contracts.md
invariants:
- 'INV-DUAL-SURFACE-PARITY: Operations available on both surfaces share semantic projections and one formatting engine; equivalent data/options produce identical Markdown and equivalent parsed JSON. Existing CLI-only operations remain CLI-only.'
- 'INV-MARKDOWN-FIRST-DEFAULT: MCP tools and CLI default to high-density Markdown; explicit format: "json" and --format json preserve programmatic output under ADR 0019, amending ADR 0001 Decision 2.'
- 'INV-DEAD-METADATA-ELIMINATION: Omit inactive presentation metadata and empty optional graph sections through operation-aware compaction; preserve all active nested/source continuations and durable/domain data.'
- 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve all actionable workflow guidance, stage rules, evidence identities, applicable invariants, dependencies, allowed write scopes, subtask DoD, and receipts under existing operation bounds.'
- 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Replace duplicated boilerplate only with equivalent concise contextual guidance; preserve supported recovery steps and error semantics.'
- 'INV-CLICKABLE-COORDINATES: Render available file, symbol, and document coordinates as absolute file:/// links with real line fragments; resolve against the selected workspace and never invent missing coordinates.'
related_plans:
- docs/adr/0019-mcp-markdown-output-and-dead-metadata-compaction.md
- docs/plans/forge-mcp-audit-and-architecture.md
started_at: 2026-10-20T22:26:18+00:00
---

# Markdown-First Dual-Surface Architecture, Rich Code Intelligence, and Agent Ergonomics

## Outcome and authority

Implement the admitted Markdown default and shared formatting architecture in
[ADR 0019](../adr/0019-mcp-markdown-output-and-dead-metadata-compaction.md),
with field semantics in the
[Markdown contracts](../reference/mcp-markdown-contracts.md).

The user authorizes M054 implementation and contract consistency reconciliation.
The Markdown amendment to ADR 0001 is admitted. This contract preserves the
original feature commitments: manifest-backed ecosystem components, active
milestones and documentation hubs, resolution health, enriched symbol context,
verified graph/test paths, Git-change impact, document diagnostics, language-aware
AST alerts, task guidance and coordination. Missing producer data is implemented
by its admitted owner or explicitly held for authority; it is never invented by
a formatter.

## Execution order and boundaries

Run the six tasks in the dependency order below. Domain formatters share
`src/core/format/`, CLI dispatch, and MCP schemas, so their work is serialized.
The first task makes format selection and safe generic rendering executable for
all existing public responses; later tasks add domain presentations. Parity
verification is last. Do not add a persisted response-format setting, storage
schema, new MCP operation, test-intent inference, or new lifecycle semantics.

Compaction reuses/moves the existing canonical pure machinery in
`src/mcp/metadata.rs` into core as needed, retaining compatibility re-exports.
Preserve core leaf isolation and existing reader, path, SQLite, error and
response-budget policies. Determine token removal after byte trimming; preserve
generation for source-only and independently nested continuations. Never
recursively delete fields by name. Checkpoint user data, SQL rows, durable
gate/candidate/content hashes, receipts and actionable guidance remain intact.

Equivalent requests compare shared semantic values and output content. Transport
framing/newlines and actual runtime/transport identity differences are normalized
explicitly in tests; no normalization may hide missing domain fields.
CLI-only milestone, recovery and maintenance operations remain CLI-only.

## Presentation acceptance

The following admitted presentation improvements belong to successor tasks;
they do not change the active core task specification:

- Discovery renders verified inspect governing rules as an IMPORTANT callout
  and overview health with same-scope granular 053 reason counts. Preserve the
  external-excluded denominator, separate import/reference units from package
  counts, and mark unavailable data explicitly. Supply missing reason aggregates
  through bounded reuse of the canonical diagnostic classifier, not page totals.
- Traversal renders verified test paths as linked test -> helper -> target
  breadcrumbs, hydrating only returned IDs for verified roles/coordinates.
  Explain/impact distinguish `[proven call]` from `[heuristic / candidate]`
  using actual edge kind/confidence and call-site coordinates; governing rules
  use the same IMPORTANT callout without losing supplied rules or bounds.
- Lifecycle renders embedded blackboard blockers in claim/context, including
  contract-stage context, as CAUTION BLOCKER callouts with original identities,
  bodies and existing recovery. Coordination applies the same presentation to
  standalone blackboard while preserving distinct nonblocker topics.

The [reference examples](../reference/mcp-markdown-contracts.md#presentation-examples)
bind field mappings, fixture coordinates, count arithmetic and proof sensitivity.
The overview lacks baseline reason aggregates and test paths carry IDs without
per-hop metadata; discovery and traversal respectively own these bounded producer
additions in their existing scopes. Neither gap authorizes invented data or a
different resolver, role classifier, storage representation or evidence standard.

## Deferred uncertainty

`Post-054 architecture / next planning / test intent`: The proposed
`positive_seam|negative_seam` badges require a producer and verification rule
that the baseline does not supply. The complete baseline
`src/db/symbols/test_discovery.rs` emits verified `test_role` and
`connection.relation/call_path` (lines 231-260), and returns discovery method
and unresolved-reference caveats (lines 396-402); it emits no intent field.
Language profiles provide roles, not positive/negative intent.

The user explicitly defers positive/negative intent for review after 054.
M054 implements verified roles, call paths, breadcrumbs and coordinates only;
it introduces neither intent inference nor a declaration producer and emits no
intent badges. Revisit the unresolved semantics, producer ownership and
verification rule, and required proof in architecture / next planning after 054.
This recorded uncertainty does not block traversal.

Git-change impact is available: `src/mcp/tools/inputs.rs` accepts `changes`
and `src/db/traversal.rs` lines 917-1018 produces changed symbols, reached
nodes, tests, unmapped changes, completeness, and change-view generation.
It requires presentation and preservation, not a new Git-diff capability.

## Proof policy

Use seam-test-first and the active profile's returned workflow guidance. For
each task record a focused production-seam command, failing/passing counts,
sensitivity mutation, full affected-target counts, and strict Clippy exit code.
Extend existing domain seam functions additively and bind their actual selectors
at Contract. Future labels below identify proof areas, not authority for standalone
duplicates. Create compiling production-connected scaffolding only for genuinely
new boundaries.

| Task | Production path and focused proof | Required affected targets | Sensitivity |
| --- | --- | --- | --- |
| Core | CLI / MCP dispatch -> shared formatter -> bounded response; `cargo test --test mcp context::` includes checkpoint opacity: `repair_checkpoint_page_shapes_are_opaque_and_persistence_is_unchanged`; SQL-column preservation: `stdio_analyze_preserves_sql_columns_named_like_metadata`; format selection and source continuation: `stdio_default_pages_and_stale_continuation_are_explicit`; byte-limited continuation: `stdio_byte_pruning_continues_after_exactly_the_emitted_rows` | `cargo test --test mcp`; `cargo test --test cli_roots`; `cargo test --test core`; additional affected targets for JSON opt-in harness edits | Remove generation after tail trimming or source-only continuation; explicit JSON/default/invalid format or opaque-content assertion fails |
| Discovery | Search/inspect/overview/AST caller -> live producer -> shared domain formatter; `cargo test --test query_context markdown_discovery_contract` | `cargo test --test query_context`; `cargo test --test mcp`; `cargo test --test languages` when AST/profile/scanner producer changes | Mislabel a directory as a crate, omit a supplied governing rule, aggregate only a visible page, include external references in health, or use Rust diagnostic examples for Python; seam fails |
| Traversal | Impact/explain/tests/analyze caller -> real graph/Git/docs evidence -> formatter; `cargo test --test impact_context markdown_traversal_contract` | `cargo test --test impact_context`; `cargo test --test query_context`; `cargo test --test mcp` | Upgrade possible_calls to proven, reorder/drop a breadcrumb helper, infer a role, omit a governing rule, incompleteness, fallback warning or nested token; seam fails |
| Lifecycle | Claim/context/inspect/submit -> task engine/store -> formatter; `cargo test --test mcp markdown_task_guidance_contract` | `cargo test --test mcp`; `cargo test --test acdd` | Remove one rule, scope, evidence identity, DoD, receipt or embedded contract blocker body/recovery; semantic preservation assertion fails |
| Coordination | Blackboard/docs/checkpoint caller -> persisted data -> formatter; `cargo test --test mcp markdown_coordination_contract` | `cargo test --test mcp`; `cargo test --test acdd` | Drop checkpoint user values or blocker identity/gate/body/recovery, relabel a nonblocker, or invent reply/timestamp; seam fails |
| Parity | CLI process and MCP stdio -> same fixtures/results -> shared formatter; `cargo test --test mcp markdown_json_dual_surface_parity` | `cargo test --test mcp`; `cargo test --test cli_roots` | Bypass shared formatter on one surface or hide semantic data in normalization; parity fails |

Every Rust build also runs
`cargo clippy --all-targets --all-features -- -D warnings`.
No benchmarks are admitted. Final documentation scan, independent integration
review, binding, and full milestone gates follow [TESTS.md](../../TESTS.md).

## Tasks in this milestone

### task: core-dual-surface-formatter-and-metadata-compaction

```yaml
task_ref: core-dual-surface-formatter-and-metadata-compaction
target: Implement shared Markdown/JSON format selection, generic lossless rendering, and safe operation-aware compaction across existing MCP and CLI boundaries
proof_policy: seam-test-first
contract_revision: 2
invariants:
- 'INV-DUAL-SURFACE-PARITY: Shared operations use one semantic projection and formatter.'
- 'INV-MARKDOWN-FIRST-DEFAULT: Omission selects Markdown; explicit JSON works; invalid formats reject before operation execution.'
- 'INV-DEAD-METADATA-ELIMINATION: Preserve active nested/source tokens after trimming and all durable/domain/user data.'
scope:
- src/core/format/
- src/core/mod.rs
- src/core/response.rs
- src/mcp/
- src/cli/mod.rs
- tests/mcp/
- tests/mcp_context/
- tests/common/
- tests/cli_roots.rs
- tests/core/storage_batches.rs
- tests/acdd/
- docs/reference/mcp-tools.md
- docs/reference/cli.md
- docs/reference/mcp-markdown-contracts.md
- docs/adr/0019-mcp-markdown-output-and-dead-metadata-compaction.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/
status: completed
subtasks:
- subtask_ref: core-markdown-engine
  title: Wire core module; reuse/move canonical pure compaction; add shared format selection and generic rendering without downstream core imports
  status: completed
  evidence: 'cargo test --lib: exit 0, 96 passed. `cargo test --test mcp context:: -- --nocapture`: exit 0, 28 passed, including default/explicit formatting and shared response rendering. Strict `cargo clippy --all-targets --all-features -- -D warnings`: exit 0.'
- subtask_ref: dead-metadata-and-hash-pruning
  title: Compact only recognized operation metadata; preserve durable/user values and active nested/source generation after byte trimming
  status: completed
  evidence: '`cargo test --lib`: exit 0, 96 passed, including metadata compaction invariants. `cargo test --test mcp context:: -- --nocapture`: exit 0, 28 passed, covering inactive metadata, nested/source continuation tokens, false/zero, durable fields, and byte-trim continuation. Active-token suppression mutant was detected by the existing byte-pruning seam at runtime (exit 101) and restored exactly.'
- subtask_ref: dual-format-switcher-dispatch
  title: Wire schemas, strict argument allowlists, nonpaged/task/guide/checkpoint inputs, MCP response/error boundaries and CLI global output selection
  status: completed
  evidence: '`cargo test --test mcp`: exit 0, 91 passed; `cargo test --test cli_roots`: exit 0, 7 passed; `cargo test --test acdd`: exit 0, 116 passed. `cargo test --test core`: exit 0, 44 passed, 1 ignored. Omitted-format default mutation was detected by the existing checkpoint CLI/MCP seam at runtime (exit 101) and restored exactly.'
- subtask_ref: core-formatter-seam-tests
  title: Prove defaults/explicit formats/invalid-before-mutation, byte-limited tail continuation, source-only/nested pages and opaque user values through production responses; adjust existing JSON consumers explicitly
  status: completed
  evidence: '`cargo test --test mcp context:: -- --nocapture`: exit 0, 28 passed; `cargo test --test mcp`: exit 0, 91 passed. Sensitivity: default-format mutant and active-continuation-token mutant each caused runtime assertion failures (exit 101); no compile-failure red. Both production files were restored to the exact pre-mutation hashes.'
receipt:
  contract_revision: 2
  passed_at: 2026-10-21T01:17:09.234380767+00:00
  evidence:
    command_proof:
      command: cargo test --lib --test mcp --test acdd --test core --test cli_roots
      exit_code: 0
      tests_passed: 354
      tests_failed: 0
      log: 'Strict `cargo clippy --all-targets --all-features -- -D warnings` passed first (exit 0). Then the unfiltered affected-target command above passed: lib 96; acdd 116; cli_roots 7; core 44 passed/1 ignored; mcp 91; aggregate 354 passed, 0 failed, 1 ignored. Both commands used CARGO_TARGET_DIR=/home/oleksii/ContextUnity/projects/contextunity-forge-mcp/.worktrees/054-core-dual-surface-formatter/target and CONTEXTUNITY_FORGE_TEST_GLOBAL_SKILL_DIR=/tmp/m054-contextunity-forge-skill. R2 selected/auto effective-policy sensitivity and separate R3 root [] and {} omission sensitivities each produced actual runtime exit 101 and were exactly restored; details/hashes on task blackboard note 699. Refreshed Forge index generation 2c4b94ca, 353 files, 8681 nodes, 16369 edges, 714 doc sections, 0 parse errors, source_inventory_matched.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Reviewed cumulative candidate 1cf1840de8cbb80fbaeb21f9334b2076f160a13e against baseline e37a659694386df07cd048186c2a2a6004fad607 and admitted extensions tests/AGENTS.md and src/core/tasks/acdd.default.yaml. Every production/test/document change is within the allowed perimeter and implements an admitted invariant or existing seam. The untracked milestone manifest is coordinator/import state outside this scoped snapshot. Final tracked diff against candidate is empty; three new scoped file blobs match exactly (formatter 928f656e333ceef429db4285f13686b78c1bd305, reference 06d59fe080e7d2c31f4d5e886040fd785b51497b, ADR0019 397d5604d260dc235787aef02f9e7a30f593b19d). HEAD remains baseline on task/054-core-dual-surface-formatter; git diff --check exit 0. No review source, test, documentation, staging, or commit writes.
        claims:
          applicable: true
          evidence: 'PASS. Cumulative source and actual runtime seam review confirms shared canonical core formatter/projection, Markdown omission/explicit selection, explicit JSON parity, exhaustive MCP schemas/allowlists, CLI format routing and preserved public exports. No hidden global format state or core dependency on CLI/MCP. R1 is closed: canonical headings, inspect/explain compaction options, recursive object-key determinism and array/path order preservation, with actual CLI/MCP wire Markdown comparisons and only separately asserted exact top-level admission-freshness normalization (board690). R2 is closed: invalid-format errors use applicable response policy via existing effective-root resolver for primary, explicit selected and active-claim auto-selected graph operations, preserve INVALID_FORMAT/isError and remain <=1024 actual escaped wire bytes before index/task/checkpoint mutation. R3 is closed: empty root []/{} render explicitly through existing generic fallback, covered through actual MCP/CLI default/explicit Markdown/JSON and unchanged persistence; adjacent empty string/null/false/zero preserved. Operation-aware compaction retains opaque SQL/checkpoint/domain data, durable guidance/evidence/receipts and every active opaque nested/source continuation after byte trimming, with original offset/resume/oversize semantics. Original monolith removal and all changed test consumers reviewed as admitted additive seam migration, not weakened assertions. Independent full command cargo test --lib --test mcp --test acdd --test core --test cli_roots exited 0: lib96, acdd116, cli_roots7, core44+1ignored, mcp91 =354 passed/0 failed/1 ignored, archive d2c8fee9135499df. Isolated worktree target and isolated skill env used. Accepted Build strict all-target/all-feature Clippy exit0 reused (board699); previous and current restoration-bound runtime101 sensitivities are recorded boards689/699.'
        concurrency:
          applicable: true
          evidence: PASS. Reviewed task-bound immutable snapshot, distinct reviewer/build ownership and current claim8/contract2 fencing. Formatting/compaction is request-local and adds no mutable global state, database transaction, lock-order, revision, receipt, or lifecycle publication changes. Active-claim auto-root resolution reuses the existing canonical resolver and validates format before executing domain operations. Existing ACDD lifecycle/concurrency and MCP suites pass unfiltered. Builder and coordinator source writers remain frozen. Independent Cargo proof used worktree-local target to exclude unrelated M022 shared-target artifact replacement; candidate bytes were checked before and after tests. Delivery remains governed by existing atomic receipt and snapshot gates.
        project_isolation:
          applicable: true
          evidence: PASS. Formatting coordinate roots and graph operation policies use existing selected workspace/root resolution. Invalid selector/config keeps existing format-first error identity without graph/storage mutation; task/guide/checkpoint policy remains primary under existing routing. Explicit and active-claim selected-worktree seam now verifies selected response ceiling, no new selected index, and exact checkpoint/task/index bytes unchanged on rejection, while baseline normal routing/confined paths continue to pass. No new routing semantics, linked-project profile consumption, workspace identifiers, symlink/traversal behavior, or namespace/storage changes. All work and proof ran in dedicated core worktree with isolated target/skill location.
        administration:
          applicable: true
          evidence: PASS. No new migration, schema, protected profile pin, gate reference, receipt format, transaction or privileged Git implementation changes. The admitted default task YAML/guidance preserves workflow/evidence identities and all task boundaries. Historical accepted Contract evidence is preserved; decision667 governs removed-monolith proof naming, with no reset/rebind/history rewriting. Accepted decisions and future owners are durable on boards690/685/686; dedicated domain layouts, positive/negative intent, and ADR0001/index remain deferred to their authorized successors. Candidate is still baseline-parent-bound on the dedicated task branch. Existing supported Deliver with auto_commit true must write the atomic receipt and single task commit and is authorized for this same independent reviewer; no manual duplicate commit.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-DUAL-SURFACE-PARITY: Operations available on both surfaces share semantic projections and one formatting engine; equivalent data/options produce identical Markdown and equivalent parsed JSON. Existing CLI-only operations remain CLI-only.'
    - 'INV-MARKDOWN-FIRST-DEFAULT: MCP tools and CLI default to high-density Markdown; explicit format: "json" and --format json preserve programmatic output under ADR 0019, amending ADR 0001 Decision 2.'
    - 'INV-DEAD-METADATA-ELIMINATION: Omit inactive presentation metadata and empty optional graph sections through operation-aware compaction; preserve all active nested/source continuations and durable/domain data.'
    - 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve all actionable workflow guidance, stage rules, evidence identities, applicable invariants, dependencies, allowed write scopes, subtask DoD, and receipts under existing operation bounds.'
    - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Replace duplicated boilerplate only with equivalent concise contextual guidance; preserve supported recovery steps and error semantics.'
    - 'INV-CLICKABLE-COORDINATES: Render available file, symbol, and document coordinates as absolute file:/// links with real line fragments; resolve against the selected workspace and never invent missing coordinates.'
    - 'INV-DUAL-SURFACE-PARITY: Shared operations use one semantic projection and formatter.'
    - 'INV-MARKDOWN-FIRST-DEFAULT: Omission selects Markdown; explicit JSON works; invalid formats reject before operation execution.'
    - 'INV-DEAD-METADATA-ELIMINATION: Preserve active nested/source tokens after trimming and all durable/domain/user data.'
    architectural_notes:
    - 'decisions: User resumed M054. Withdraw blocker664 as an engine-blocker claim after exact source verification by original contract author: gates.rs1353-1366 validates seam_test_ref only for Contract proof; Command/Build1368-1379 validates accepted_boundary_callers and public signature scope. accepted_boundary_callers173-199 reads immutable stored callers, public_callable_nodes142-153 excludes tests. Historical seam ref in context519-535 is guidance, not current Build validation. Preserve accepted Contract claim2 snapshot d4a12e8 and red evidence unchanged, retain refactor map and independent91of91 proof as current coverage. Ordinary Build under claim3/contract2 m054-core-builder is supported; no reset, rebind, revision or policy change needed. Build rejection/reset would remain at Build and unnecessarily consume claim. Main builder runs transient sensitivity checks with exact source restoration before final build proof; no new standalone tests. Refactor finding remains independently confirmed fixed.'
    - 'decisions: Review claim4/candidate a5c41bc: external unattributed tests/AGENTS.md change added only acdd-forge:test-invariants start/end HTML comments, candidate blob14becc2a0a61068db930d21de27a3b29c8f49ec4 -> live71bae55e5b23cc396b93cb4a61f62e52c63e8003. All team writers frozen, production/test bodies unchanged. Latest file preserved verbatim /tmp/m054-core-tests-agents-live-markers.md and immutable Git blob71bae55e5b23cc396b93cb4a61f62e52c63e8003. Public Review reject enforces snapshot equality (src/engine/tasks/mod.rs874-896), so coordinator will restore only this file to original candidate for same reviewer rejection, then same builder restores preserved marker-bearing file during Build. Original verified findings and 354/0 test evidence remain bound a5c41bc. No changes discarded; marker attribution remains external/unknown.'
    - 'decisions: Parallel bounded contract scan found current manifest proof table still prescribing user-deleted monolith; original manifest author corrected ONLY proof prose/table, independently PASS blob9f81d25cf77bff91ef454f3c17a3c253f5ffbefe (prior e554ba9911ee25f9a76fc79d17904276ca3190d2). Actual four existing context seams named; focused cargo test --test mcp context::; future labels are proof areas requiring existing additive selectors at Contract, scaffolds only genuinely new boundaries. Root transferred exact2hunks into core coordinator-owned manifest blobd1eff4b482c0cdabb80af2ab68d402c5be724d96; bounded transfer check pending. Task specs/revisions/statuses/frontmatter remain unchanged, no sync needed; historical Contract/red evidence remains immutable. Discovery readiness map preserved board680. Final core proof uses isolated worktree/target because unrelated M022 Cargo process shares repository/target; never stop unrelated job.'
    - 'decisions: Fresh Build candidate572218f266d417edab4ffe43df93471002609376 accepted claim5/contractrev2; all team writers frozen. Same stable reviewer m054-core-reviewer owns closure of M054-CORE-R1/R2 and Review→Deliver if clean. Accepted bounded parity context: existing MCP top-level source-admission freshness differs from CLI existing-index behavior per runbook; preserve actual production envelope and assert exact semantic envelope/actual Markdown block separately before normalizing only that recognized top-level runtime block. No recursive stripping/core-only reconstructed wire or new CLI freshness semantics. Future domain presentations stay successor-owned. User deferred positive/negative intent to post054 architecture/nextplanning; canonical docs independently PASS, traversalrev4 syncedready, roles/paths/breadcrumbs remain; details685–686. ADR0001 amendment/index in milestone sibling remain finalparity/handoff-owned; no scope bypass. No manual admission/task commits: activeprofile auto_commit owns single delivery commit+receipt on dedicated task branch. Preserve external test/profile guidance and milestone sibling rootAGENTS edits; don''t stop unrelated M022 Cargo job. Final accepted Build used isolated coreworktree/target,354passed0failed1ignored,strictClippy0; future final review uses same isolatedtarget.'
    - 'decisions: Integration safety prepared while core Review frozen: milestone branch milestone/054-markdown-first-dual-surface-and-agent-ergonomics worktree is clean; exact8 reviewed-doc/external-guidance files safely retained in new stash4bab5ee70d79eafab60c8f9bb304abb660368340 (name m054-before-core-integration-reviewed-docs-and-external-guidance-20261021). No core candidate changes. After core delivery FF-merge taskbranch into milestone, preserve generated receipt/currentmanifest from delivery; restore ONLY still-unintegrated reviewed ADR0001, ADRindex and external rootAGENTS as worktree edits from this stash, verify already-delivered YAML/test guidance match stash rather than overwrite candidate. Retain all safety stashes (also original52c0ae95 and c0666e42), no stashdrop or unrelated primary docs restore. Core branch/worktree cleanup only after integration and preservation checks.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

The test/harness scope, including `tests/core/storage_batches.rs`, permits explicit
JSON opt-in for existing tests that parse JSON, plus contract proof for the
default switch. It does not authorize unrelated test refactors. Core task
documentation describes the switch and generic
preservation; dedicated domain layout documentation co-evolves in later tasks.

### task: code-discovery-markdown-contracts

```yaml
task_ref: code-discovery-markdown-contracts
target: "Implement inspect/search/overview/AST Markdown, verified invariant callouts, scoped health/reason counts, bounded enrichment, manifest components, milestone/doc hubs and profile diagnostics"
depends_on:
  - core-dual-surface-formatter-and-metadata-compaction
proof_policy: seam-test-first
contract_revision: 3
invariants:
  - 'INV-CLICKABLE-COORDINATES: Resolve real coordinates against the selected worktree.'
  - 'INV-DISCOVERY-EVIDENCE: Components, enrichment, health and diagnostic examples come from actual bounded producers.'
  - 'INV-DISCOVERY-PRESENTATION: Render verified governing rules in IMPORTANT callouts and canonical scoped reason counts; exclude external status from health, preserve actual units and unavailable data.'
scope:
  - src/core/format/
  - src/db/symbols.rs
  - src/db/symbols/
  - src/db/reader.rs
  - src/engine/scanner/
  - src/engine/ast/
  - src/engine/languages/
  - src/engine/tasks/context.rs
  - src/mcp/
  - src/cli/mod.rs
  - src/cli/ast.rs
  - tests/query_context/
  - tests/mcp/
  - tests/mcp_context/
  - tests/languages/
  - docs/reference/mcp-tools.md
  - docs/reference/cli.md
  - docs/reference/mcp-markdown-contracts.md
  - skills/contextunity-forge/SKILL.md
status: ready
subtasks:
  - subtask_ref: inspect-markdown-formatter
    title: "Render verified DocGraph rules as IMPORTANT Governing Invariants bullets with source links and omission metadata; preserve ADRs/relations/source options and paging"
    status: ready
  - subtask_ref: search-markdown-formatter-with-enrichment
    title: "Supply and render bounded prod/test classification, docstrings, graph pulse and exact-match reasons without fabricating evidence"
    status: ready
  - subtask_ref: overview-markdown-formatter
    title: "Supply manifest components/milestone/doc hubs and same-scope health plus four canonical 053 reason aggregates, ambiguity/unknown and separate external-reference counts; unavailable package counts remain explicit"
    status: ready
  - subtask_ref: ast-grep-markdown-and-diagnostics
    title: "Render matches/captures and profile-provided syntax diagnostics, spans when available, examples and actionable hints"
    status: ready
  - subtask_ref: code-discovery-seam-tests
    title: "Prove discovery content/links, invariant callouts, manifest ecosystems, all reason-count mappings and external-excluded arithmetic/zero denominator, bounded full-scope aggregation and two language diagnostics"
    status: ready
```

### task: graph-traversal-and-impact-markdown-contracts

```yaml
task_ref: graph-traversal-and-impact-markdown-contracts
target: "Render explain/impact/tests/analyze with linked verified test breadcrumbs, explicit exact/candidate edge badges, governing-rule callouts and Git/docs evidence"
depends_on:
  - code-discovery-markdown-contracts
proof_policy: seam-test-first
contract_revision: 4
invariants:
  - 'INV-TRAVERSAL-EVIDENCE: Preserve exact versus lexical evidence, incompleteness, Git-change fingerprint continuation and verified documentation.'
  - 'INV-CLICKABLE-COORDINATES: Render only actual symbol and edge coordinates.'
  - 'INV-TRAVERSAL-PRESENTATION: Preserve ordered verified path roles/links, exact calls versus possible/lexical evidence, and supplied governing rules in IMPORTANT callouts without fabricated call sites.'
scope:
  - src/core/format/
  - src/db/reader.rs
  - src/db/traversal.rs
  - src/db/symbols.rs
  - src/db/symbols/test_discovery.rs
  - src/mcp/
  - src/cli/mod.rs
  - tests/impact_context.rs
  - tests/query_context/
  - tests/mcp/
  - docs/reference/mcp-tools.md
  - docs/reference/cli.md
  - docs/reference/mcp-markdown-contracts.md
  - skills/contextunity-forge/SKILL.md
status: ready
subtasks:
  - subtask_ref: impact-markdown-formatter
    title: "Render depth, actual call-site links, proven calls versus candidate badges and verified IMPORTANT governing-rule callouts, affected docs and Git-change incompleteness"
    status: ready
  - subtask_ref: explain-markdown-formatter
    title: "Render signature/docstring, verified governing-rule callouts when supplied, real call-site links and exact/candidate badges with inbound/outbound paging"
    status: ready
  - subtask_ref: tests-chains-markdown-formatter
    title: "Render linked test_case -> verified helpers -> target breadcrumbs using bounded returned-ID hydration; preserve unknown roles/coordinates and graph/lexical caveats"
    status: ready
  - subtask_ref: analyze-diagnostics-markdown-formatter
    title: "Render existing untested public-surface, syntax and documentation drift diagnostics with supported remediation"
    status: ready
  - subtask_ref: traversal-seam-tests
    title: "Prove breadcrumb fixture order/roles/real links, calls versus possible_calls/dynamic/lexical badges, governing callouts, Git/docs incompleteness and nested continuation"
    status: ready
```

### task: task-lifecycle-markdown-contracts-and-bundle-compaction

```yaml
task_ref: task-lifecycle-markdown-contracts-and-bundle-compaction
target: "Render task lifecycle and embedded claim/contract blackboard blocker callouts while preserving all operative guidance, DoD, evidence identities and receipts"
depends_on:
  - graph-traversal-and-impact-markdown-contracts
proof_policy: seam-test-first
contract_revision: 3
invariants:
  - 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve task authority, stage guidance, proof commands/templates, write scopes, DoD and durable evidence.'
  - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Compact duplicate presentation while preserving every distinct operative instruction.'
  - 'INV-EMBEDDED-BLOCKER-PRESENTATION: Claim/context including contract-stage blackboard uses CAUTION BLOCKER callouts with complete message identity, author, scope, gate and supplied body/recovery; nonblocker topics remain distinct.'
scope:
  - src/core/format/
  - src/engine/tasks/context.rs
  - src/db/tasks_store/lifecycle.rs
  - src/mcp/
  - src/cli/mod.rs
  - src/cli/task.rs
  - tests/mcp/
  - tests/acdd/
  - docs/reference/tasks.md
  - docs/reference/acdd.md
  - docs/reference/mcp-tools.md
  - docs/reference/cli.md
  - docs/reference/mcp-markdown-contracts.md
  - skills/contextunity-forge/SKILL.md
  - skills/contextunity-forge/references/
status: ready
subtasks:
  - subtask_ref: task-context-bundle-markdown-formatter
    title: "Render each stage/minimal mode under ADR 0016; embedded claim/contract blockers use shared CAUTION callouts preserving identities, gate, supplied body and existing full-body recovery"
    status: ready
  - subtask_ref: task-inspect-and-manage-markdown-formatter
    title: "Render full inspect history and manage/submit outcomes without removing available findings, candidate bindings or receipts"
    status: ready
  - subtask_ref: task-list-markdown-formatter
    title: "Render hierarchy/status/dependencies and cancellation accurately"
    status: ready
  - subtask_ref: task-lifecycle-seam-tests
    title: "Compare authority/evidence across formats/stages and prove contract claim blocker/nonblocker callouts with long identity, gate, multiline body, summary recovery and continuations"
    status: ready
```

### task: coordination-and-documentation-markdown-contracts

```yaml
task_ref: coordination-and-documentation-markdown-contracts
target: "Render standalone blackboard CAUTION BLOCKER callouts, documentation and checkpoints with original identities, body/recovery, continuation and existing action semantics"
depends_on:
  - task-lifecycle-markdown-contracts-and-bundle-compaction
proof_policy: seam-test-first
contract_revision: 3
invariants:
  - 'INV-COORDINATION-CONTENT: Preserve message/document/checkpoint content, actual actions and hierarchy; invent no threading, timestamps or state.'
  - 'INV-CLICKABLE-COORDINATES: Resolve actual document and symbol coordinates.'
  - 'INV-BLOCKER-PRESENTATION: Only blockers receive CAUTION BLOCKER (#id) by author callouts; preserve complete identity, gate, supplied body or exact inspect recovery and every page continuation.'
scope:
  - src/core/format/
  - src/db/tasks_store/blackboard.rs
  - src/db/reader.rs
  - src/engine/docs.rs
  - src/mcp/
  - src/cli/mod.rs
  - src/cli/task.rs
  - src/cli/guide.rs
  - tests/mcp/
  - tests/acdd/
  - docs/reference/tasks.md
  - docs/reference/mcp-tools.md
  - docs/reference/cli.md
  - docs/reference/mcp-markdown-contracts.md
  - skills/contextunity-forge/SKILL.md
status: ready
subtasks:
  - subtask_ref: blackboard-threaded-markdown-formatter
    title: "Group actual topics chronologically; blockers render CAUTION BLOCKER (#id) by author with scope/gate/time/body or exact bounded inspect recovery, preserving continuation and distinct nonblockers"
    status: ready
  - subtask_ref: doc-tools-markdown-formatter
    title: "Render actual document sections/outlines/search excerpts and verified symbol links with detail/continuation"
    status: ready
  - subtask_ref: checkpoint-markdown-formatter
    title: "Render list byte sizes and save/get/delete outcomes losslessly; preserve arbitrary user content and separate checkpoint size behavior"
    status: ready
  - subtask_ref: coordination-seam-tests
    title: "Prove standalone blocker/nonblocker identity/gate/body/recovery/paging and shared embedded callout semantics, persisted docs/checkpoints and opaque user values across formats"
    status: ready
```

### task: cli-surface-parity-json-alignment-and-seam-tests

```yaml
task_ref: cli-surface-parity-json-alignment-and-seam-tests
target: "Complete and verify shared-operation Markdown/JSON parity, supported errors and default selection across real CLI and MCP seams"
depends_on:
  - coordination-and-documentation-markdown-contracts
proof_policy: seam-test-first
contract_revision: 2
invariants:
  - 'INV-DUAL-SURFACE-PARITY: Shared operations produce equivalent semantic results and use the same formatter; CLI-only operations retain their boundary.'
  - 'INV-DEAD-METADATA-ELIMINATION: Shared compaction preserves active recovery and all meaningful domain evidence.'
scope:
  - src/core/format/
  - src/cli/
  - src/mcp/
  - tests/cli_roots.rs
  - tests/common/
  - tests/mcp/
  - docs/adr/0001-dual-surface-parity.md
  - docs/adr/0019-mcp-markdown-output-and-dead-metadata-compaction.md
  - docs/reference/cli.md
  - docs/reference/mcp-tools.md
  - docs/reference/mcp-markdown-contracts.md
  - skills/contextunity-forge/SKILL.md
  - skills/contextunity-forge/references/
status: ready
subtasks:
  - subtask_ref: cli-markdown-wiring
    title: "Close remaining shared-operation wiring gaps and preserve readable CLI-only output without new MCP operations"
    status: ready
  - subtask_ref: json-payload-compaction-and-alignment
    title: "Prove equivalent parsed JSON with explicit documented transport/runtime normalization and protected domain fields"
    status: ready
  - subtask_ref: dual-surface-snapshot-parity-tests
    title: "Exercise all registered tools and existing CLI equivalents through real processes, both formats, defaults, continuations and errors"
    status: ready
```
