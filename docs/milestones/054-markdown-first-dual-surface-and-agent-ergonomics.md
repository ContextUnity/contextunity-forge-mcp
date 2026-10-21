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

Core and discovery retain their accepted execution and proof boundaries. Run
the four remaining tasks concurrently in separate worktrees. Their declared
dependencies retain receipt and integration ordering; they do not prohibit
authorized parallel implementation. Integrate shared formatter, CLI dispatch,
MCP schema and test changes before consolidated verification of the merged
content against every task contract. Parity verification covers that combined
content. Do not add a persisted response-format setting, storage schema, new MCP
operation, test-intent inference, or new lifecycle semantics.

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

Core and discovery retain seam-test-first and their accepted proof history. The
four remaining tasks use the supported `deferred-final-test` policy and the
active profile's returned workflow guidance. Contract may verify an existing
production seam with its actual exit code 0; do not invent a failing baseline.
Record actual focused seam results and runtime sensitivity evidence. Extend
existing domain seam functions additively and bind their actual selectors at
Contract. Future labels below identify proof areas, not authority for standalone
duplicates. Create compiling production-connected scaffolding only for genuinely
new boundaries.

Defer the remaining tasks' full affected-target suites and strict Clippy to
consolidated verification after all four implementations are merged. Bind the
passing commands, actual counts, sensitivity results and independent integration
review to the merged content and each applicable task contract. Separate branch
full-suite runs are not required; focused branch checks report only what they
actually verify. Preserve every acceptance claim and required affected target
below, and run the full milestone gates on the final combined content.

| Task | Production path and focused proof | Required affected targets | Sensitivity |
| --- | --- | --- | --- |
| Core | CLI / MCP dispatch -> shared formatter -> bounded response; `cargo test --test mcp context::` includes checkpoint opacity: `repair_checkpoint_page_shapes_are_opaque_and_persistence_is_unchanged`; SQL-column preservation: `stdio_analyze_preserves_sql_columns_named_like_metadata`; format selection and source continuation: `stdio_default_pages_and_stale_continuation_are_explicit`; byte-limited continuation: `stdio_byte_pruning_continues_after_exactly_the_emitted_rows` | `cargo test --test mcp`; `cargo test --test cli_roots`; `cargo test --test core`; additional affected targets for JSON opt-in harness edits | Remove generation after tail trimming or source-only continuation; explicit JSON/default/invalid format or opaque-content assertion fails |
| Discovery | Search/inspect/overview/AST caller -> live producer -> shared domain formatter; `cargo test --test query_context markdown_discovery_contract` | `cargo test --test query_context`; `cargo test --test mcp`; `cargo test --test languages` when AST/profile/scanner producer changes | Mislabel a directory as a crate, omit a supplied governing rule, aggregate only a visible page, include external references in health, or use Rust diagnostic examples for Python; seam fails |
| Traversal | Impact/explain/tests/analyze caller -> real graph/Git/docs evidence -> formatter; `cargo test --test impact_context markdown_traversal_contract` | `cargo test --test impact_context`; `cargo test --test query_context`; `cargo test --test mcp` | Upgrade possible_calls to proven, reorder/drop a breadcrumb helper, infer a role, omit a governing rule, incompleteness, fallback warning or nested token; seam fails |
| Lifecycle | Claim/context/inspect/submit -> task engine/store -> formatter; `cargo test --test mcp markdown_task_guidance_contract` | `cargo test --test mcp`; `cargo test --test acdd` | Remove one rule, scope, evidence identity, DoD, receipt or embedded contract blocker body/recovery; semantic preservation assertion fails |
| Coordination | Blackboard/docs/checkpoint caller -> persisted data -> formatter; `cargo test --test mcp markdown_coordination_contract` | `cargo test --test mcp`; `cargo test --test acdd` | Drop checkpoint user values or blocker identity/gate/body/recovery, relabel a nonblocker, or invent reply/timestamp; seam fails |
| Parity | CLI process and MCP stdio -> same fixtures/results -> shared formatter; `cargo test --test mcp markdown_json_dual_surface_parity` | `cargo test --test mcp`; `cargo test --test cli_roots` | Bypass shared formatter on one surface or hide semantic data in normalization; parity fails |

Consolidated verification runs every required affected target above and
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
target: Implement inspect/search/overview/AST Markdown, verified invariant callouts, scoped health/reason counts, bounded enrichment, manifest components, milestone/doc hubs and profile diagnostics
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
status: completed
subtasks:
- subtask_ref: inspect-markdown-formatter
  title: Render verified DocGraph rules as IMPORTANT Governing Invariants bullets with source links and omission metadata; preserve ADRs/relations/source options and paging
  status: completed
  evidence: 'R1 post-repair actual MCP wire proof: `cargo test --test mcp freshness::stdio_and_response::mcp_document_retrieval_and_explain_output_controls` passed 1, failed 0 (archive 141ebe539e016b4b). Existing JSON fixture proves omitted_transitive=1 and bounded omitted_sources; the same Markdown assertion now verifies the omission count and every source ID/path/line file link while preserving direct rule, JSON, and continuation assertions.'
- subtask_ref: search-markdown-formatter-with-enrichment
  title: Supply and render bounded prod/test classification, docstrings, graph pulse and exact-match reasons without fabricating evidence
  status: completed
  evidence: 'Historical whole-candidate proof at 447442e33743a37e3761886ab2d9f3d5198fb7a9: unfiltered `cargo test --test query_context --test mcp --test languages` passed 397 (query_context 43, MCP 91, languages 263; 0 failed/ignored; archive 6b43ebb49cf88b42), including actual MCP search enrichment/ranking. R1 only changes inspect invariant omission rendering and its existing MCP seam; search implementation is unchanged.'
- subtask_ref: overview-markdown-formatter
  title: Supply manifest components/milestone/doc hubs and same-scope health plus four canonical 053 reason aggregates, ambiguity/unknown and separate external-reference counts; unavailable package counts remain explicit
  status: completed
  evidence: 'Historical whole-candidate proof at 447442e33743a37e3761886ab2d9f3d5198fb7a9: the same unfiltered command passed query_context 43/43 (archive 6b43ebb49cf88b42), including `diagnostics_and_source::overview_reports_language_breakdown_and_compiled_profiles` over real reader output and the pure renderer. The overview implementation/test are unchanged by R1.'
- subtask_ref: ast-grep-markdown-and-diagnostics
  title: Render matches/captures and profile-provided syntax diagnostics, spans when available, examples and actionable hints
  status: completed
  evidence: 'Historical whole-candidate proof at 447442e33743a37e3761886ab2d9f3d5198fb7a9: the unfiltered command passed MCP 91/91 and languages 263/263 (archive 6b43ebb49cf88b42), including existing actual MCP AST and language-diagnostic seams. R1 changes only inspect invariant metadata rendering and the associated inspect test.'
- subtask_ref: code-discovery-seam-tests
  title: Prove discovery content/links, invariant callouts, manifest ecosystems, all reason-count mappings and external-excluded arithmetic/zero denominator, bounded full-scope aggregation and two language diagnostics
  status: completed
  evidence: 'Historical candidate 447442e33743a37e3761886ab2d9f3d5198fb7a9 unfiltered proof: 397 passed, 0 failed/ignored (archive 6b43ebb49cf88b42). R1-specific post-repair actual MCP wire test passed 1/1, 0 failed (archive 141ebe539e016b4b), asserting supplied omitted_transitive and omitted_sources are preserved. Do not interpret the historical 397 as post-R1 evidence; root owns consolidated full verification after parallel-task integration.'
receipt:
  contract_revision: 3
  passed_at: 2026-10-21T03:45:52.002878404+00:00
  evidence:
    command_proof:
      command: cargo test --test mcp freshness::stdio_and_response::mcp_document_retrieval_and_explain_output_controls
      exit_code: 0
      tests_passed: 1
      tests_failed: 0
      log: 'R1 focused actual MCP wire regression passed 1, failed 0, 90 filtered; archive 141ebe539e016b4b. The test checks the JSON producer''s omitted_transitive=1 and bounded omitted_sources, then asserts the Markdown omission count and every ID/path/line coordinate link. rustfmt targeted check and git diff --check exit 0. Current tracked diff SHA-256 7f6f6e6ee8860811a35b97b63a7990921e7b388b5cd9bb40bf50c5ba38235c1b; src/core/format/discovery.rs SHA-256 dc65da14789de3b6dc86e474494456339922654c0512cd73ee11190eaf7f5cf5; tests/mcp/freshness/stdio_and_response.rs SHA-256 622ca70affbe3646e10ddb8656b6364130303cdc37fdc215b177772e66407e33. Historical candidate 447442e33743a37e3761886ab2d9f3d5198fb7a9 only: strict Clippy 0 and full query_context/MCP/languages 397 passed, 0 failed/ignored, archive 6b43ebb49cf88b42. No full-suite or Clippy result is claimed for the R1-modified candidate; root will run consolidated verification after concurrent task integration under user override. Coordinator-approved lifecycle manifest amendment is unchanged and provenance is recorded on board748. Five subtasks were updated via Forge with explicit bounded evidence; no manual manifest or receipt edits.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Review claim7/contract3 binds frozen f45fcea873c39c4e523b623981d13de9f37dc5fd to baseline24b35b4961dfea8e01cff6eb30c3c113d95a6a3d. Exact snapshot-to-snapshot delta from independently reviewed447442e consists only src/core/format/discovery.rs and additive lines within existing mcp_document_retrieval_and_explain_output_controls. Live tracked admitted files match frozen snapshot; new formatter blobc9d827f05e7c440211a887455111e152fe39f2c8 and scanner blob9ecdb66bf4a2fa1377ce58b670cacf061da5774e match candidate. git diff --check0. Cumulative scope/path review board750 remains applicable to unchanged content. Approved coordinator-only lifecycle rev4 manifest amendment remains separately recorded719/748. No reviewer source edits, staging, index refresh or manual commits.
        claims:
          applicable: true
          evidence: 'PASS. M054-DISCOVERY-R1 CLOSED: render_inspect734–770 consumes existing positive omitted_transitive and every bounded omitted_sources entry, preserving supplied ID/path/line/URI and truthful coordinate/link-unavailable fallback. Existing actual raw MCP inspect seam296–312 asserts producer-bound omission count and each exact source row; JSON/direct/provenance/get_doc recovery baselines218–267 remain unchanged. Builder focused actual MCP command cargo test --test mcp freshness::stdio_and_response::mcp_document_retrieval_and_explain_output_controls exit0,1passed/0failed/0ignored/90filtered, archive141ebe539e016b4b independently inspected; rustfmt and diffcheck0 board757. Other prior D-P1/D-P2/search-heading/overview-aspects/empty-coverage-status/JSON baseline closures preserved because all other source/test bytes match447442e. HISTORICAL ONLY: independent447442e unfiltered query_context43+mcp91+languages263=397/0/0 archive8acba5903f4df015; accepted447442e strictClippy0 archiveee1588700c88b154. These are not represented as postrepair full/lint results. User execution override752/755 explicitly consolidates complete verification after integration; no extra branch full/lint run. Current review template requires decision and five contours, not fabricated command proof. Final merged all-contract strictClippy/full verification remains coordinator-owned pending work.'
        concurrency:
          applicable: true
          evidence: PASS. Bounded repair is pure rendering of an already supplied immutable Value plus assertions; no producer, global state, storage, snapshot admission, QueryBudget, task revision, transaction, locks or receipt implementation changes. Unchanged cumulative read-only bounded aggregation and request-local formatting retain prior verified architecture. Candidate was frozen before claim and live source reconciled; concurrent remaining builders use separate worktrees per explicit override752 and do not write this candidate. No Cargo or source mutation was performed in this Review.
        project_isolation:
          applicable: true
          evidence: PASS. Existing adapter selected/linked-root canonical guards and Markdown-only URI decoration remain byte-identical to447442e; JSON receives no added URI or speculative query fields. Repair prints only supplied validated URI/path/line and explicit unavailable fallbacks, with no invented coordinate lookup. Same existing actual MCP fixture verifies expected source links against its real selected workspace. Manifest inventory bounds/ignore/linked behavior unchanged; historical full target evidence remains expressly historical.
        administration:
          applicable: true
          evidence: PASS. No new producer/schema/migration/protected profile pin/gate/receipt/privileged Git semantics. Same independent reviewer m054-discovery-reviewer differs from builder m054-discovery-builder; claim7/rev3 exact template used. Builder reconciled all five existing subtasks through Forge with evidence board757, no reviewer manual metadata edits. Root authorized Review→Deliver in this same session with auto_commit owning atomic receipt/single task commit. Positive/negative intent and later presentation owners remain deferred/unchanged, and user consolidation override752 is durable before delivery. No remaining admitted finding; R1 closed, prior closed clusters preserved, final integration verification explicitly pending rather than claimed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-DUAL-SURFACE-PARITY: Operations available on both surfaces share semantic projections and one formatting engine; equivalent data/options produce identical Markdown and equivalent parsed JSON. Existing CLI-only operations remain CLI-only.'
    - 'INV-MARKDOWN-FIRST-DEFAULT: MCP tools and CLI default to high-density Markdown; explicit format: "json" and --format json preserve programmatic output under ADR 0019, amending ADR 0001 Decision 2.'
    - 'INV-DEAD-METADATA-ELIMINATION: Omit inactive presentation metadata and empty optional graph sections through operation-aware compaction; preserve all active nested/source continuations and durable/domain data.'
    - 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve all actionable workflow guidance, stage rules, evidence identities, applicable invariants, dependencies, allowed write scopes, subtask DoD, and receipts under existing operation bounds.'
    - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Replace duplicated boilerplate only with equivalent concise contextual guidance; preserve supported recovery steps and error semantics.'
    - 'INV-CLICKABLE-COORDINATES: Render available file, symbol, and document coordinates as absolute file:/// links with real line fragments; resolve against the selected workspace and never invent missing coordinates.'
    - 'INV-CLICKABLE-COORDINATES: Resolve real coordinates against the selected worktree.'
    - 'INV-DISCOVERY-EVIDENCE: Components, enrichment, health and diagnostic examples come from actual bounded producers.'
    - 'INV-DISCOVERY-PRESENTATION: Render verified governing rules in IMPORTANT callouts and canonical scoped reason counts; exclude external status from health, preserve actual units and unavailable data.'
    architectural_notes:
    - 'decisions: Exact source-scope treatment before final Build: new physical src/cli/discovery.rs not in allowed scope (only cli/mod.rs,cli/ast.rs). Original manifest author + independent m054-discovery-reviewer both read full source rules/helper/callsites and PASS same-behavior standard INLINE pub(crate) mod discovery within admitted cli/mod.rs, retaining cohesive module namespace/functions/visibility/callers. Frozen preimage helperf5330eb39faff57af7a1bddf52abdff3d904d272 (201lines), mod0cc17f50fca7cbc7940d6f439882f61b35cd1b58. src/AGENTS45/50/56–63/88 no router-only or arbitraryline/physicalfile requirement; responsibility separation retained inline. Same builder authorized exact body-preserving lexical move and removal of only new physical file, token/body comparison modulo indent, normal final proof after move. No scope extension attempted: existing ready parity src/cli/ ownership conflicts with physicalfile extension under store rules. No new semantic functionality, manifest/spec/revision/digest/claim/gates/reset/sync change justified. This is authority adjudication only, NOT frozen candidate gateReview. Final independent Review binds actual resulting candidate.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: graph-traversal-and-impact-markdown-contracts

```yaml
task_ref: graph-traversal-and-impact-markdown-contracts
target: Render explain/impact/tests/analyze with linked verified test breadcrumbs, explicit exact/candidate edge badges, governing-rule callouts and Git/docs evidence
depends_on:
- code-discovery-markdown-contracts
proof_policy: deferred-final-test
contract_revision: 5
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
status: completed
subtasks:
- subtask_ref: impact-markdown-formatter
  title: Render depth, actual call-site links, proven calls versus candidate badges and verified IMPORTANT governing-rule callouts, affected docs and Git-change incompleteness
  status: completed
  evidence: |-
    {
      "command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast",
      "exit_code": 0,
      "tests_passed": 261,
      "tests_failed": 0,
      "archive_id": "f5fe4011bc1785ca",
      "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d",
      "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4",
      "targets": {
        "acdd": 116,
        "cli_roots": 7,
        "impact_context": 4,
        "mcp": 91,
        "query_context": 43
      },
      "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."
    }
- subtask_ref: explain-markdown-formatter
  title: Render signature/docstring, verified governing-rule callouts when supplied, real call-site links and exact/candidate badges with inbound/outbound paging
  status: completed
  evidence: |-
    {
      "command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast",
      "exit_code": 0,
      "tests_passed": 261,
      "tests_failed": 0,
      "archive_id": "f5fe4011bc1785ca",
      "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d",
      "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4",
      "targets": {
        "acdd": 116,
        "cli_roots": 7,
        "impact_context": 4,
        "mcp": 91,
        "query_context": 43
      },
      "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."
    }
- subtask_ref: tests-chains-markdown-formatter
  title: Render linked test_case -> verified helpers -> target breadcrumbs using bounded returned-ID hydration; preserve unknown roles/coordinates and graph/lexical caveats
  status: completed
  evidence: |-
    {
      "command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast",
      "exit_code": 0,
      "tests_passed": 261,
      "tests_failed": 0,
      "archive_id": "f5fe4011bc1785ca",
      "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d",
      "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4",
      "targets": {
        "acdd": 116,
        "cli_roots": 7,
        "impact_context": 4,
        "mcp": 91,
        "query_context": 43
      },
      "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."
    }
- subtask_ref: analyze-diagnostics-markdown-formatter
  title: Render existing untested public-surface, syntax and documentation drift diagnostics with supported remediation
  status: completed
  evidence: |-
    {
      "command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast",
      "exit_code": 0,
      "tests_passed": 261,
      "tests_failed": 0,
      "archive_id": "f5fe4011bc1785ca",
      "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d",
      "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4",
      "targets": {
        "acdd": 116,
        "cli_roots": 7,
        "impact_context": 4,
        "mcp": 91,
        "query_context": 43
      },
      "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."
    }
- subtask_ref: traversal-seam-tests
  title: Prove breadcrumb fixture order/roles/real links, calls versus possible_calls/dynamic/lexical badges, governing callouts, Git/docs incompleteness and nested continuation
  status: completed
  evidence: |-
    {
      "command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast",
      "exit_code": 0,
      "tests_passed": 261,
      "tests_failed": 0,
      "archive_id": "f5fe4011bc1785ca",
      "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d",
      "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4",
      "targets": {
        "acdd": 116,
        "cli_roots": 7,
        "impact_context": 4,
        "mcp": 91,
        "query_context": 43
      },
      "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."
    }
receipt:
  contract_revision: 5
  passed_at: 2026-10-21T04:48:28.467010979+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast
      exit_code: 0
      tests_passed: 261
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Independent claim7/revision5 binds scoped snapshot494f759bd4a4c8dfad2a2c72afb15e424e9f3d23 to baseline83ac02b8a33887199959f1b46b475832f9ca9de4. Live admitted files match candidate exactly. Cumulative integration review used explicit target basee37a659694386df07cd048186c2a2a6004fad607 and frozen source treeb3e1406e22221bf79e8c0eeb8872915c0825281d, patcha4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4. No untracked source or diffcheck errors. Peer changes retain their admitted owners; exact six-path documentation closure belongs parityrev4. Generated subtask metadata alone changed after source verification. Formatter remains a pure leaf; returned-ID hydration belongs db/symbols/test_discovery.rs. No reviewer edits, staging, Cargo, index refresh, configuration or main-worktree mutation.
        claims:
          applicable: true
          evidence: 'PASS. Traced explain/impact/tests/analyze/query adapters through bounded graph/Git/docs producers into shared traversal renderer. Proven calls require actual kind/confidence; possible/lexical edges retain candidate caveats. Verified test breadcrumbs preserve returned-ID order, actual roles/coordinates and missing-call-site warnings. Governing rules, diagnostics, Git-change identity, independent nested/source continuations and remaining supplied fields are retained. Actual CLI/MCP wire seams compare omitted/explicit Markdown exactly and parsed JSON, separately asserting only actual top-level server freshness before normalization; no hidden domain loss. AST common limits1..100 share canonical page data; larger CLI-only aggregate range remains explicit. Existing seams extended additively. Consolidated cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast exited0: strictClippy0,261passed/0failed/0ignored (ACDD116 CLI7 impact4 MCP91 query43), archivef5fe4011bc1785ca/board780, bound frozen source. Traversal exact-badge and missing-call-site-caveat production mutations caused actual runtime failures085786a71149b7c2; exact restoration verified. Invalid prior lifecycle-label sensitivity excluded; actual parsed-context/rules sensitivity separately recorded778/c2aed0b394ae63b5. No historical branch count or failed run is labelled current pass. Full milestone handoff gates remain pending coordinator.'
        concurrency:
          applicable: true
          evidence: PASS. Formatting is request-local over immutable supplied Values, without global format state or changes to transactions, revision fencing, claim ownership, reader snapshot semantics or lock ordering. Existing admission, shared locks, generation/baseline comparisons, rollback and receipt implementation remain authoritative. Hydration is bounded to returned graph IDs. All source writers frozen for positive checks and review; deliberate mutations restored byte-exact before positive proof. Same independent reviewer continues through Deliver. No reviewer writer or Cargo lane introduced.
        project_isolation:
          applicable: true
          evidence: PASS. Existing selected/linked-root canonical adapter/scanner guards remain authoritative. Presentation-only URI decoration uses actual supplied coordinates; no JSON enrichment or invented call sites. Core has no filesystem/downstream adapter IO. Task/project namespaces, profile ownership, symlink/traversal confinement and malformed-input-before-mutation semantics remain intact. Actual selected-root/CLI/MCP seams passed within MCP91/CLI7. Review/Deliver confined to dedicated M054 milestone worktree; unrelated concurrent M060/main changes excluded and untouched.
        administration:
          applicable: true
          evidence: PASS. Workerm054-integration-reviewer differs from accepted builderm054-traversal-builder. Returned claim-bound template and all five configured contours used. No migration/new operation/protected profile pin/gate/configuration change, historical receipt rewrite or privileged Git action. User override752 and supported deferred-final-test amendment758 are durable; traversalrev5 unchanged. Positive/negative intent remains deferred post054 with no invented producer/badges. Prior admitted integration findings closed or accurately corrected, current documentation rescan passed per coordinator; actual source/reference paths agree. Five subtasks completed viaForge. Deliver must preserve candidate equality and use this same independent worker; auto_commit owns atomic receipt/single scoped branch commit, no duplicate manual commit.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-DUAL-SURFACE-PARITY: Operations available on both surfaces share semantic projections and one formatting engine; equivalent data/options produce identical Markdown and equivalent parsed JSON. Existing CLI-only operations remain CLI-only.'
    - 'INV-MARKDOWN-FIRST-DEFAULT: MCP tools and CLI default to high-density Markdown; explicit format: "json" and --format json preserve programmatic output under ADR 0019, amending ADR 0001 Decision 2.'
    - 'INV-DEAD-METADATA-ELIMINATION: Omit inactive presentation metadata and empty optional graph sections through operation-aware compaction; preserve all active nested/source continuations and durable/domain data.'
    - 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve all actionable workflow guidance, stage rules, evidence identities, applicable invariants, dependencies, allowed write scopes, subtask DoD, and receipts under existing operation bounds.'
    - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Replace duplicated boilerplate only with equivalent concise contextual guidance; preserve supported recovery steps and error semantics.'
    - 'INV-CLICKABLE-COORDINATES: Render available file, symbol, and document coordinates as absolute file:/// links with real line fragments; resolve against the selected workspace and never invent missing coordinates.'
    - 'INV-TRAVERSAL-EVIDENCE: Preserve exact versus lexical evidence, incompleteness, Git-change fingerprint continuation and verified documentation.'
    - 'INV-CLICKABLE-COORDINATES: Render only actual symbol and edge coordinates.'
    - 'INV-TRAVERSAL-PRESENTATION: Preserve ordered verified path roles/links, exact calls versus possible/lexical evidence, and supplied governing rules in IMPORTANT callouts without fabricated call sites.'
    architectural_notes:
    - 'deferred: {"authority":"Explicit user decision: відклади це, зафіксуй як невизначеність щоб переглянути потім","item":"positive_seam / negative_seam intent badges","disposition":"Deferred outside M054; no longer a traversal implementation blocker after contract/store reconciliation","current_delivery":"Verified test_role, ordered call_path/breadcrumbs, actual coordinates and graph-vs-lexical caveats remain fully in M054","uncertainty":["Meaning and nonexclusive positive/negative or mixed cases","Declared author intent versus verified behavioral proof","Per-test intent versus target-specific edge intent","Producer and language support","Provenance precision, missing/malformed policy and measurable sensitivity proof"],"baseline":"Existing code supplies roles and path evidence but no positive/negative intent producer; original ADR example assumed otherwise","destination":"Post-M054 architecture/contract planning review; no producer, annotation syntax, schema or inference mechanism admitted now","next":"Same original manifest author updates only future traversal rev3→4 and docs; independent consistency review, coordinator transfer and Forge sync; core task rev2 remains unchanged"}'
    - 'decisions: User deferral685 reconciled and independently reviewed PASS. Sibling manifest9e924044fe28abc54c5e892c67d3314ed4897d9a / refde3bff2bc787514ca2d2a56c444343c7dbcd7ac0; core composed manifest4e638cc69557fd0bb0398f657ac3820d2fdce095 / ref06d59fe080e7d2c31f4d5e886040fd785b51497b. Core ref also retains builder freshness note under separate implementation review. Supported CLI task sync054 succeeded all6; traversal revision4 ready/contract, then supported subtask_update set existing tests-chains ready without altering evidence. Allgatespending/dependencysequence preserved. No reset/reopen/history/receipt edits. Verified core digest/rev2/claim5/owner/buildstage/live scopeextensions/accepted gatehistory/4completedsubtasks+evidence identical before/afterSync. Deferred uncertainty remains post054 architecture/nextplanning; currentverifiedroles/paths/breadcrumbs/coordinates remain required.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: task-lifecycle-markdown-contracts-and-bundle-compaction

```yaml
task_ref: task-lifecycle-markdown-contracts-and-bundle-compaction
target: Render task lifecycle and embedded claim/contract blackboard blocker callouts while preserving all operative guidance, DoD, evidence identities and receipts
depends_on:
- graph-traversal-and-impact-markdown-contracts
proof_policy: deferred-final-test
contract_revision: 5
invariants:
- 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve task authority, stage guidance, proof commands/templates, write scopes, DoD and durable evidence.'
- 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Compact duplicate presentation while preserving every distinct operative instruction.'
- 'INV-EMBEDDED-BLOCKER-PRESENTATION: Claim/context including contract-stage blackboard uses CAUTION BLOCKER callouts with complete message identity, author, scope, gate and supplied body/recovery; nonblocker topics remain distinct.'
scope:
- src/core/format/
- src/engine/tasks/context.rs
- src/engine/tasks/mod.rs
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
status: completed
subtasks:
- subtask_ref: task-context-bundle-markdown-formatter
  title: Render each stage/minimal mode under ADR 0016; embedded claim/contract blockers use shared CAUTION callouts preserving identities, gate, supplied body and existing full-body recovery
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
- subtask_ref: task-inspect-and-manage-markdown-formatter
  title: Render full inspect history and manage/submit outcomes without removing available findings, candidate bindings or receipts
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
- subtask_ref: task-list-markdown-formatter
  title: Render hierarchy/status/dependencies and cancellation accurately
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
- subtask_ref: task-lifecycle-seam-tests
  title: Compare authority/evidence across formats/stages and prove contract claim blocker/nonblocker callouts with long identity, gate, multiline body, summary recovery and continuations
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
receipt:
  contract_revision: 5
  passed_at: 2026-10-21T04:52:58.895830320+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast
      exit_code: 0
      tests_passed: 261
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Reviewed integrated source tree b3e1406e22221bf79e8c0eeb8872915c0825281d against explicit target base e37a659694386df07cd048186c2a2a6004fad607; patch SHA-256 a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4. Live tracked content matched this binding, no untracked source, and diff checks passed. Traversal delivery0941313e791620c68d9f943868f723b07263a3fc preserved reviewed scoped bytes. Remaining task content is admitted under its own current scope; peer-owned changes remain attributed to those owners. Only Forge-generated task metadata/receipts may differ from the tested tree. No reviewer source, test, documentation, configuration, staging, Cargo or index mutation. Lifecycle scope includes shared core formatting, engine task context/mod, task-store lifecycle, CLI/MCP task boundaries, existing MCP/ACDD seams and admitted task/ACDD/CLI/MCP/reference/skill documentation. Generic blackboard specialization was removed only from arbitrary nested values; actual root/bundle/guidance blackboard positions remain explicit.
        claims:
          applicable: true
          evidence: 'PASS. Traced task list/claim/context/manage/inspect/submit output through real task engine/store into the shared lifecycle renderer. Actual statuses, cancellation, dependencies, write scopes, stage guidance, proof commands/templates, DoD, candidate/content/gate identities, histories and durable receipts are retained. Equal repeated guidance is deduplicated only after value equality; distinct operative instructions remain. Embedded blockers retain supplied contextual scope (including sibling), identity, author, gate, time, supplied body or exact full-message recovery; standalone canonical references are fallback only when scope was not supplied. Nonblocker topics retain their distinct data. Findings are preserved in both actual producer shapes: Build context parses unresolved_review_findings into objects, while completed inspect returns the complete opaque stored JSON string. Existing raw MCP lifecycle seam checks real rejection-to-Build context JSON equality and Markdown arbitrary blackboard/blackboard_messages values, plus completed-inspect full stored string, parsed semantic equality, candidate and receipt retention. Unsupported inspect nested-label assumptions were withdrawn, not converted into new production parsing. Source SHA5419391e0e0421436979fa37021bcaf3364edbbb7a380d562bcf33483cdad909 and test SHA93e0c8f223d9c0ab1945b581af2d9348c5f24249cfe1cb2402214648cd6cc1f7 were independently verified. Controlled rules omission and recursive arbitrary-blackboard interpretation compiled and caused exactly two actual runtime ACDD failures,10passed/2failed/exit101 archivec2aed0b394ae63b5, followed by exact restoration. Earlier lifecycle-label failure is invalid sensitivity and is excluded. Current consolidated production-seam evidence: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast exited0, strictClippy0,261passed/0failed/0ignored (ACDD116 CLI7 impact4 MCP91 query43), archivef5fe4011bc1785ca/board780. This is the shared run on reviewed b3e source under user override752 and supported deferred-final-test amendment758; no per-task rerun or historical branch result is claimed. Full milestone handoff gates remain coordinator-owned.'
        concurrency:
          applicable: true
          evidence: PASS. Request-local formatting over supplied immutable Values introduces no global format state. Existing storage transactions, claim/revision fencing, candidate baseline equality, reader locks, source admission, rollback and receipt implementation remain authoritative. No lock-order, shared-database namespace, migration or mutation behavior was changed by the presentation work. Writers were frozen for positive verification and review; deliberate mutations were restored byte-exactly before the positive run. Concurrent implementations were isolated then integrated under explicit user authority. Metadata-only gate transport must preserve the reviewed source and accepted current candidate.
        project_isolation:
          applicable: true
          evidence: PASS. Existing canonical selected/linked-root scanner and adapter guards remain authoritative. Core formatting performs no filesystem or downstream adapter IO. CLI/MCP presentation adapters attach only real supplied coordinates against the selected workspace; no fabricated URI/call site or JSON enrichment. Existing repository/project task namespaces, profile resolution, traversal/symlink confinement and malformed-input-before-mutation behavior remain intact. Actual CLI/MCP/root seams passed within the shared MCP91/CLI7 targets. All gate work remains in the dedicated M054 milestone worktree; unrelated primary-main M060 content is excluded and untouched.
        administration:
          applicable: true
          evidence: PASS. Actual independent reviewer is the same stable session/worker m054-integration-reviewer, distinct from the implementation authors. Coordinator may transport this unchanged attributed proof inside the current accepted claim's exact template; transport is not a new self-review. No new MCP operation, storage schema, protected profile/gate/configuration semantics, historical receipt rewrite or privileged Git action. Core/discovery accepted histories remain intact. Current documentation scan and exact parity-only six-path amendment were independently reviewed; no extra functionality is admitted. Positive/negative test intent remains deferred post054. Any final receipt is generated by Forge; auto_commit owns the single scoped delivery commit, with no manual duplicate. Final candidate/claim binding must be reconciled against this source before submission. Lifecycle Contract claim5 and Build claim6 accepted at revision5; all four subtasks complete. Formal existing seam stdio_analyze_preserves_sql_columns_named_like_metadata in tests/mcp/context.rs has an actual indexed one-hop path to compact_mcp_metadata for nested/source/durable preservation. Actual ACDD116 raw-wire lifecycle checks and the two-failure mutation remain explicit material rendering proof; no standalone ACDD external-process graph edge, synthetic graph connection or accepted path beyond the verified seam is claimed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-DUAL-SURFACE-PARITY: Operations available on both surfaces share semantic projections and one formatting engine; equivalent data/options produce identical Markdown and equivalent parsed JSON. Existing CLI-only operations remain CLI-only.'
    - 'INV-MARKDOWN-FIRST-DEFAULT: MCP tools and CLI default to high-density Markdown; explicit format: "json" and --format json preserve programmatic output under ADR 0019, amending ADR 0001 Decision 2.'
    - 'INV-DEAD-METADATA-ELIMINATION: Omit inactive presentation metadata and empty optional graph sections through operation-aware compaction; preserve all active nested/source continuations and durable/domain data.'
    - 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve all actionable workflow guidance, stage rules, evidence identities, applicable invariants, dependencies, allowed write scopes, subtask DoD, and receipts under existing operation bounds.'
    - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Replace duplicated boilerplate only with equivalent concise contextual guidance; preserve supported recovery steps and error semantics.'
    - 'INV-CLICKABLE-COORDINATES: Render available file, symbol, and document coordinates as absolute file:/// links with real line fragments; resolve against the selected workspace and never invent missing coordinates.'
    - 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve task authority, stage guidance, proof commands/templates, write scopes, DoD and durable evidence.'
    - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Compact duplicate presentation while preserving every distinct operative instruction.'
    - 'INV-EMBEDDED-BLOCKER-PRESENTATION: Claim/context including contract-stage blackboard uses CAUTION BLOCKER callouts with complete message identity, author, scope, gate and supplied body/recovery; nonblocker topics remain distinct.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: coordination-and-documentation-markdown-contracts

```yaml
task_ref: coordination-and-documentation-markdown-contracts
target: Render standalone blackboard CAUTION BLOCKER callouts, documentation and checkpoints with original identities, body/recovery, continuation and existing action semantics
depends_on:
- task-lifecycle-markdown-contracts-and-bundle-compaction
proof_policy: deferred-final-test
contract_revision: 4
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
status: completed
subtasks:
- subtask_ref: blackboard-threaded-markdown-formatter
  title: Group actual topics chronologically; blockers render CAUTION BLOCKER (#id) by author with scope/gate/time/body or exact bounded inspect recovery, preserving continuation and distinct nonblockers
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
- subtask_ref: doc-tools-markdown-formatter
  title: Render actual document sections/outlines/search excerpts and verified symbol links with detail/continuation
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
- subtask_ref: checkpoint-markdown-formatter
  title: Render list byte sizes and save/get/delete outcomes losslessly; preserve arbitrary user content and separate checkpoint size behavior
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
- subtask_ref: coordination-seam-tests
  title: Prove standalone blocker/nonblocker identity/gate/body/recovery/paging and shared embedded callout semantics, persisted docs/checkpoints and opaque user values across formats
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
receipt:
  contract_revision: 4
  passed_at: 2026-10-21T04:53:22.387064543+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast
      exit_code: 0
      tests_passed: 261
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Reviewed integrated source tree b3e1406e22221bf79e8c0eeb8872915c0825281d against explicit target base e37a659694386df07cd048186c2a2a6004fad607; patch SHA-256 a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4. Live tracked content matched this binding, no untracked source, and diff checks passed. Traversal delivery0941313e791620c68d9f943868f723b07263a3fc preserved reviewed scoped bytes. Remaining task content is admitted under its own current scope; peer-owned changes remain attributed to those owners. Only Forge-generated task metadata/receipts may differ from the tested tree. No reviewer source, test, documentation, configuration, staging, Cargo or index mutation. Coordination scope includes shared core formatting, existing blackboard store/document reader/docs engine, CLI/MCP task/guide boundaries, MCP/ACDD seams and admitted task/CLI/MCP/reference/skill docs. Renderer and explicit checkpoint action routing remain inside those owners.
        claims:
          applicable: true
          evidence: 'PASS. Traced standalone blackboard, docs and checkpoint operations from persisted data through actual CLI/MCP boundaries. Blackboard groups actual topics without fabricated threading/replies/timestamps, retaining producer order within groups, complete supplied identity/scope/gate/time/body, empty-body distinction, exact summary inspect recovery and pagination. Only blockers receive CAUTION BLOCKER treatment, shared with the embedded lifecycle helper. Document search/get retain actual supplied section content/excerpts/outlines, available line links, verified citations/symbol relations, ambiguous navigation candidates and independent continuation metadata. Meaningful actual server freshness is emitted on both doc paths; only recognized full/compacted top-level envelopes receive that treatment, without filtering arbitrary domain fields. Checkpoint action is explicit: list reports actual serialized byte sizes, save/delete retain actual action/name, get uses lossless generic rendering for arbitrary values including empty collections, false, zero, null, nested metadata-like keys and user bytes fields. Original validation, atomic writes,1MiB storage limit and separate oversized response behavior remain unchanged. Actual MCP doc-delta freshness and two checkpoint opacity/size production mutations produced runtime failures in085786a71149b7c2; all were restored exactly. Invalid historical lifecycle-label sensitivity is excluded. Existing persisted-data/raw-wire seams passed in the consolidated run. Current consolidated production-seam evidence: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast exited0, strictClippy0,261passed/0failed/0ignored (ACDD116 CLI7 impact4 MCP91 query43), archivef5fe4011bc1785ca/board780. This is the shared run on reviewed b3e source under user override752 and supported deferred-final-test amendment758; no per-task rerun or historical branch result is claimed. Full milestone handoff gates remain coordinator-owned.'
        concurrency:
          applicable: true
          evidence: PASS. Request-local formatting over supplied immutable Values introduces no global format state. Existing storage transactions, claim/revision fencing, candidate baseline equality, reader locks, source admission, rollback and receipt implementation remain authoritative. No lock-order, shared-database namespace, migration or mutation behavior was changed by the presentation work. Writers were frozen for positive verification and review; deliberate mutations were restored byte-exactly before the positive run. Concurrent implementations were isolated then integrated under explicit user authority. Metadata-only gate transport must preserve the reviewed source and accepted current candidate.
        project_isolation:
          applicable: true
          evidence: PASS. Existing canonical selected/linked-root scanner and adapter guards remain authoritative. Core formatting performs no filesystem or downstream adapter IO. CLI/MCP presentation adapters attach only real supplied coordinates against the selected workspace; no fabricated URI/call site or JSON enrichment. Existing repository/project task namespaces, profile resolution, traversal/symlink confinement and malformed-input-before-mutation behavior remain intact. Actual CLI/MCP/root seams passed within the shared MCP91/CLI7 targets. All gate work remains in the dedicated M054 milestone worktree; unrelated primary-main M060 content is excluded and untouched.
        administration:
          applicable: true
          evidence: PASS. Actual independent reviewer is the same stable session/worker m054-integration-reviewer, distinct from the implementation authors. Coordinator may transport this unchanged attributed proof inside the current accepted claim's exact template; transport is not a new self-review. No new MCP operation, storage schema, protected profile/gate/configuration semantics, historical receipt rewrite or privileged Git action. Core/discovery accepted histories remain intact. Current documentation scan and exact parity-only six-path amendment were independently reviewed; no extra functionality is admitted. Positive/negative test intent remains deferred post054. Any final receipt is generated by Forge; auto_commit owns the single scoped delivery commit, with no manual duplicate. Final candidate/claim binding must be reconciled against this source before submission. No new freshness producer, reply model, restoration effect or checkpoint semantics is introduced.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-DUAL-SURFACE-PARITY: Operations available on both surfaces share semantic projections and one formatting engine; equivalent data/options produce identical Markdown and equivalent parsed JSON. Existing CLI-only operations remain CLI-only.'
    - 'INV-MARKDOWN-FIRST-DEFAULT: MCP tools and CLI default to high-density Markdown; explicit format: "json" and --format json preserve programmatic output under ADR 0019, amending ADR 0001 Decision 2.'
    - 'INV-DEAD-METADATA-ELIMINATION: Omit inactive presentation metadata and empty optional graph sections through operation-aware compaction; preserve all active nested/source continuations and durable/domain data.'
    - 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve all actionable workflow guidance, stage rules, evidence identities, applicable invariants, dependencies, allowed write scopes, subtask DoD, and receipts under existing operation bounds.'
    - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Replace duplicated boilerplate only with equivalent concise contextual guidance; preserve supported recovery steps and error semantics.'
    - 'INV-CLICKABLE-COORDINATES: Render available file, symbol, and document coordinates as absolute file:/// links with real line fragments; resolve against the selected workspace and never invent missing coordinates.'
    - 'INV-COORDINATION-CONTENT: Preserve message/document/checkpoint content, actual actions and hierarchy; invent no threading, timestamps or state.'
    - 'INV-CLICKABLE-COORDINATES: Resolve actual document and symbol coordinates.'
    - 'INV-BLOCKER-PRESENTATION: Only blockers receive CAUTION BLOCKER (#id) by author callouts; preserve complete identity, gate, supplied body or exact inspect recovery and every page continuation.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

### task: cli-surface-parity-json-alignment-and-seam-tests

```yaml
task_ref: cli-surface-parity-json-alignment-and-seam-tests
target: Complete and verify shared-operation Markdown/JSON parity, supported errors and default selection across real CLI and MCP seams
depends_on:
- coordination-and-documentation-markdown-contracts
proof_policy: deferred-final-test
contract_revision: 4
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
- README.md
- AGENTS.md
- docs/architecture/README.md
- docs/reference/README.md
- docs/runbooks/limits-and-freshness.md
- docs/adr/README.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/
status: completed
subtasks:
- subtask_ref: cli-markdown-wiring
  title: Close remaining shared-operation wiring gaps and preserve readable CLI-only output without new MCP operations
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
- subtask_ref: json-payload-compaction-and-alignment
  title: Prove equivalent parsed JSON with explicit documented transport/runtime normalization and protected domain fields
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
- subtask_ref: dual-surface-snapshot-parity-tests
  title: Exercise all registered tools and existing CLI equivalents through real processes, both formats, defaults, continuations and errors
  status: completed
  evidence: '{"command": "cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast", "exit_code": 0, "tests_passed": 261, "tests_failed": 0, "archive_id": "f5fe4011bc1785ca", "tree": "b3e1406e22221bf79e8c0eeb8872915c0825281d", "patch_sha256": "a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4", "targets": {"acdd": 116, "cli_roots": 7, "impact_context": 4, "mcp": 91, "query_context": 43}, "notes": "User-authorized concurrent isolated builds integrated before one consolidated strict Clippy + all five full affected targets. Actual global skill matches repository. Controlled sensitivity evidence /tmp/m054-sensitivity-evidence.md; all mutations restored exact before passing run. No new per-task Cargo run."}'
receipt:
  contract_revision: 4
  passed_at: 2026-10-21T04:53:52.848768914+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast
      exit_code: 0
      tests_passed: 261
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: PASS. Reviewed integrated source tree b3e1406e22221bf79e8c0eeb8872915c0825281d against explicit target base e37a659694386df07cd048186c2a2a6004fad607; patch SHA-256 a4fdd392ea30d6171cee841b1dc05d8561ea45a4276a55962605e4b7857e47e4. Live tracked content matched this binding, no untracked source, and diff checks passed. Traversal delivery0941313e791620c68d9f943868f723b07263a3fc preserved reviewed scoped bytes. Remaining task content is admitted under its own current scope; peer-owned changes remain attributed to those owners. Only Forge-generated task metadata/receipts may differ from the tested tree. No reviewer source, test, documentation, configuration, staging, Cargo or index mutation. Parity scope covers core formatter, CLI/MCP adapters, existing CLI/common/MCP tests, ADR0001/0019 and canonical interfaces/skills. Independently approved rev3-to4 amendment admits exactly README.md,AGENTS.md,docs/architecture/README.md,docs/reference/README.md,docs/runbooks/limits-and-freshness.md,docs/adr/README.md. All other task fields, protected authority and accepted receipts were preserved; the amendment closes authorized documentation perimeter only.
        claims:
          applicable: true
          evidence: 'PASS. Traced existing shared-operation inputs/options and semantic values through one formatting/compaction path on real CLI processes and MCP stdio, with no surrogate renderer replacing actual wire output. Omitted/explicit Markdown compare exactly after admitted transport newline handling; JSON compares parsed values. Only exact actual top-level server-owned freshness is separately asserted then normalized; nested/arbitrary domain fields remain protected. Overview''s actual envelope and allowed inactive dedicated-discovery absence are explicit; meaningful notices are retained. Canonical headings, inspect/explain options and search continuation scope booleans agree. AST equivalent limits1..100 now use the same canonical matches page, pattern/language and Markdown request scope; active limit1 offset/generation/totals assertions are additive. CLI-only101..10000 aggregation remains supported and accurately documented outside MCP''s validated range. The four rows after AST share equivalent docs-search, docs-get, guide and checkpoint producers/options, with no further semantic mismatch found. Every registered MCP tool advertises optional exact markdown/json selector; defaults, invalid-before-operation, strict unknown fields, error identity/isError/CLI status, selected-root coordinates and active nested/source/byte-trim recovery remain covered by actual existing seams. MCP-only get_code_snippet is tested through its real wire boundary without inventing a CLI command. The genuine pre-fix CLI search-scope mismatch failed at runtime in archivef0a1860ba1f327f5; AST mismatch was corrected without normalizing payload away. Existing tests were extended rather than replacing domain assertions or adding duplicate standalone monoliths. Current source/reference/ADR amendment/docs/skill behavior is consistent; source preview bounds/defaults and portable links were verified. Current consolidated production-seam evidence: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test cli_roots --test impact_context --test query_context --test mcp --test acdd --no-fail-fast exited0, strictClippy0,261passed/0failed/0ignored (ACDD116 CLI7 impact4 MCP91 query43), archivef5fe4011bc1785ca/board780. This is the shared run on reviewed b3e source under user override752 and supported deferred-final-test amendment758; no per-task rerun or historical branch result is claimed. Full milestone handoff gates remain coordinator-owned.'
        concurrency:
          applicable: true
          evidence: PASS. Request-local formatting over supplied immutable Values introduces no global format state. Existing storage transactions, claim/revision fencing, candidate baseline equality, reader locks, source admission, rollback and receipt implementation remain authoritative. No lock-order, shared-database namespace, migration or mutation behavior was changed by the presentation work. Writers were frozen for positive verification and review; deliberate mutations were restored byte-exactly before the positive run. Concurrent implementations were isolated then integrated under explicit user authority. Metadata-only gate transport must preserve the reviewed source and accepted current candidate.
        project_isolation:
          applicable: true
          evidence: PASS. Existing canonical selected/linked-root scanner and adapter guards remain authoritative. Core formatting performs no filesystem or downstream adapter IO. CLI/MCP presentation adapters attach only real supplied coordinates against the selected workspace; no fabricated URI/call site or JSON enrichment. Existing repository/project task namespaces, profile resolution, traversal/symlink confinement and malformed-input-before-mutation behavior remain intact. Actual CLI/MCP/root seams passed within the shared MCP91/CLI7 targets. All gate work remains in the dedicated M054 milestone worktree; unrelated primary-main M060 content is excluded and untouched.
        administration:
          applicable: true
          evidence: PASS. Actual independent reviewer is the same stable session/worker m054-integration-reviewer, distinct from the implementation authors. Coordinator may transport this unchanged attributed proof inside the current accepted claim's exact template; transport is not a new self-review. No new MCP operation, storage schema, protected profile/gate/configuration semantics, historical receipt rewrite or privileged Git action. Core/discovery accepted histories remain intact. Current documentation scan and exact parity-only six-path amendment were independently reviewed; no extra functionality is admitted. Positive/negative test intent remains deferred post054. Any final receipt is generated by Forge; auto_commit owns the single scoped delivery commit, with no manual duplicate. Final candidate/claim binding must be reconciled against this source before submission. The six-path documentation scope amendment changes no functionality, target, proof policy, dependencies, invariants or subtask acceptance. No new transport or MCP operation is added.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-DUAL-SURFACE-PARITY: Operations available on both surfaces share semantic projections and one formatting engine; equivalent data/options produce identical Markdown and equivalent parsed JSON. Existing CLI-only operations remain CLI-only.'
    - 'INV-MARKDOWN-FIRST-DEFAULT: MCP tools and CLI default to high-density Markdown; explicit format: "json" and --format json preserve programmatic output under ADR 0019, amending ADR 0001 Decision 2.'
    - 'INV-DEAD-METADATA-ELIMINATION: Omit inactive presentation metadata and empty optional graph sections through operation-aware compaction; preserve all active nested/source continuations and durable/domain data.'
    - 'INV-PRESERVED-CONTRACT-INTEGRITY: Preserve all actionable workflow guidance, stage rules, evidence identities, applicable invariants, dependencies, allowed write scopes, subtask DoD, and receipts under existing operation bounds.'
    - 'INV-CONTEXTUAL-ACTIONABLE-HINTS: Replace duplicated boilerplate only with equivalent concise contextual guidance; preserve supported recovery steps and error semantics.'
    - 'INV-CLICKABLE-COORDINATES: Render available file, symbol, and document coordinates as absolute file:/// links with real line fragments; resolve against the selected workspace and never invent missing coordinates.'
    - 'INV-DUAL-SURFACE-PARITY: Shared operations produce equivalent semantic results and use the same formatter; CLI-only operations retain their boundary.'
    - 'INV-DEAD-METADATA-ELIMINATION: Shared compaction preserves active recovery and all meaningful domain evidence.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```
