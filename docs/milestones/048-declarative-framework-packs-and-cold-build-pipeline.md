---
id: m-declarative-framework-packs-and-cold-build-pipeline
title: Declarative framework packs, cold build pipeline optimization, and workspace alignment
doc_type: contract
status: active
depends_on:
- m-indexer-allocation-delta-and-dispatch-precision
owners:
- src/db/writer.rs
- src/engine/scanner/
- src/engine/languages/dependency_registry.rs
- src/engine/languages/manifests/
- src/engine/languages/vue.rs
- src/engine/ast/routes.rs
- src/engine/linker/
- skills/contextunity-forge/
- docs/
invariants:
- 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository copy (commerce-release-update) must not regress; eliminating redundant file I/O and secondary filesystem traversals must reduce overall cold build latency.'
- 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions by default, permitting granular rule enrichment without duplicating standard library tables.'
- 'INV-FRAMEWORK-DISCOVERY: Framework manifest rules activate when explicitly placed in .forge/frameworks/ or declared in package manifests, enabling template-driven libraries like Alpine.js, HTMX, and GridViewSpec to resolve without mandatory package.json entry.'
- 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic and pass cargo test --all-targets.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
started_at: 2026-10-16T00:37:50+00:00
---

# Declarative framework packs, cold build pipeline optimization, and workspace alignment

## Outcome and purpose

Deliver an end-to-end resolution of the architectural, documentation, and indexing gaps identified in the comprehensive ContextUnity Forge audit:
1. Align documentation, ADR records (0001, 0015, 0016), task profiles, and agent skills with current runtime reality; provide production-ready `.forge/frameworks/*.toml.example` reference manifests. (Completed in Task 1).
2. Optimize the cold build pipeline by eliminating double file reads and double SHA-256 hashing between scanner and writer, removing the redundant secondary directory walk in `DependencyRegistry` while preserving root-level manifest discovery, and pooling Tree-sitter parsers across all supported languages.
3. Upgrade framework manifest semantics to support declarative extension (deterministic merge with bundled rules where project rules take precedence), automatic activation when placed in `.forge/frameworks/`, and bundled `gridviewspec.toml`.
4. Align configuration, gitignore guards, and linked workspaces in the control repository `commerce-release-update`, `grid-view-spec`, and `traverse`.

---

## Tasks in this milestone

### task: documentation-skills-and-reference-alignment

```yaml
task_ref: documentation-skills-and-reference-alignment
target: Align documentation, task profiles, reference examples, and agent skills with verified runtime contracts and schemas
proof_policy: direct-proof
contract_revision: 1
scope:
- docs/reference/profiles.md
- docs/reference/configuration.md
- docs/reference/tasks.md
- docs/reference/framework-manifests.md
- docs/architecture/indexing.md
- README.md
- TESTS.md
- docs/testing/README.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/
- .forge/
status: completed
subtasks:
- subtask_ref: fix-docs-and-profiles-schema
  title: Correct GateProfile and ACDD profile schema examples in docs/reference/profiles.md and clarify TASK_GUIDANCE_MISSING triggers in configuration.md and tasks.md
  status: completed
  evidence: Aligned GateProfile schema in docs/reference/profiles.md, clarified TASK_GUIDANCE_MISSING in configuration.md, tasks.md, and README.md, fixed query sql and build stage references.
- subtask_ref: align-shared-skills-and-add-framework-examples
  title: Update skills/contextunity-forge/SKILL.md, fix forge-mcp.yaml.example and acdd_profile.yaml.example, add django, alpinejs, and gridviewspec reference examples, and sync installed skills
  status: completed
  evidence: Updated SKILL.md, fixed forge-mcp.yaml.example and acdd_profile.yaml.example, created django, alpinejs, and gridviewspec reference examples in references/frameworks/.
- subtask_ref: clean-obsolete-root-profile
  title: Remove obsolete .forge/acdd_profile.yaml and align README.md and TESTS.md
  status: completed
  evidence: Removed obsolete .forge/acdd_profile.yaml from workspace root.
receipt:
  contract_revision: 1
  passed_at: 2026-10-16T00:45:54.015346067+00:00
  evidence:
    command_proof:
      command: cargo test --test core
      exit_code: 0
      tests_passed: 31
      tests_failed: 0
      log: Docs, profiles, and skills aligned with verified runtime schemas
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All modified and added files stay strictly within the admitted task write perimeter.
        claims:
          applicable: true
          evidence: GateProfile schemas deserialize cleanly, TASK_GUIDANCE_MISSING triggers match workspaces.rs, and reference examples provide valid django, alpinejs, and gridviewspec manifests.
        concurrency:
          applicable: true
          evidence: Markdown and YAML edits do not touch runtime concurrency guards or locks.
        project_isolation:
          applicable: true
          evidence: Changes are strictly confined to forge-mcp.
        administration:
          applicable: true
          evidence: Removed obsolete root profile .forge/acdd_profile.yaml; active profile intact.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository copy (commerce-release-update) must not regress; eliminating redundant file I/O and secondary filesystem traversals must reduce overall cold build latency.'
    - 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions by default, permitting granular rule enrichment without duplicating standard library tables.'
    - 'INV-FRAMEWORK-DISCOVERY: Framework manifest rules activate when explicitly placed in .forge/frameworks/ or declared in package manifests, enabling template-driven libraries like Alpine.js, HTMX, and GridViewSpec to resolve without mandatory package.json entry.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic and pass cargo test --all-targets.'
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

#### Acceptance

1. `docs/reference/profiles.md` YAML examples validate against strict `GateProfile` deserialization.
2. `docs/reference/configuration.md` accurately describes `TASK_GUIDANCE_MISSING` conditions matching `workspaces.rs`.
3. `skills/contextunity-forge/references/` contains valid `forge-mcp.yaml.example`, `acdd_profile.yaml.example`, and `.forge/frameworks/*.toml.example` files.
4. Installed global skill `~/.agents/skills/contextunity-forge/` matches the updated repository skill.
5. All tests and docs checks pass.

---

### task: cold-build-pipeline-and-parser-optimization

```yaml
task_ref: cold-build-pipeline-and-parser-optimization
target: Eliminate cold build I/O bottlenecks and optimize parser pooling without regressing indexing fidelity
proof_policy: seam-test-first
contract_revision: 6
scope:
- src/engine/scanner/mod.rs
- src/engine/scanner/adapter.rs
- src/db/writer.rs
- src/db/writer/memory_budget_tests.rs
- src/engine/languages/dependency_registry.rs
- src/engine/languages/mod.rs
- src/engine/ast/mod.rs
- src/engine/languages/vue.rs
- src/engine/languages/vue/setup_facts.rs
- docs/reference/cli.md
- docs/reference/configuration.md
- docs/architecture/concurrency.md
- benchmarks/
- tests/core/storage_batches.rs
- tests/languages/typescript/frameworks.rs
status: completed
subtasks:
- subtask_ref: eliminate-double-file-read-and-digest
  title: Read and hash source files exactly once during cold build extraction within configurable RAM budget (limits.memory_budget_mb and FORGE_MEMORY_BUDGET_MB); benchmark on commerce-release-update across 50, 200, 500, 1000 MB to determine optimal throughput-to-memory operating point
  status: completed
  evidence: Final authorized 22-build series is recorded in benchmarks/results/048-cold-build.md and .json. Adaptive median was 13.551s baseline to 13.461s candidate (-0.664%); paired median change -2.359%. The 3-run 50/200/500/1000 MiB matrix selected 1000 MiB under the predeclared lowest-RSS-within-5%-of-fastest rule (12.825s median, 1,475,256,320-byte median peak RSS). Effective budget reporting matched each configured budget. All 38,818 persisted domain_commitments rows across 17 domains were bit-identical; per-binary roots were deterministic across budgets.
- subtask_ref: eliminate-dependency-registry-second-walk
  title: Collect dependency manifest paths during scanner traversal while preserving discovery for root and ancestor manifests outside source roots via direct existence checks across all language manifest filenames, eliminating redundant WalkBuilder filesystem walks
  status: completed
  evidence: Build ingestion now consumes manifest paths captured by scanner traversal and bounded direct ancestor/root probes; no redundant recursive manifest walk. Ancestor probing stops at the workspace root, skips configured ignored components, and accepts only checked non-symlink regular files. Nuxt boundary regression nuxt_autoimports_require_declared_project_and_preserve_local_shadow initially failed because plain/package.json beside plain/src was missed; after the bounded ancestor probe it passes in final cargo test --test languages --quiet (253 passed, 0 failed, 0 ignored).
- subtask_ref: expand-parser-pool-and-single-pass-vue
  title: Extend thread-local PARSER_POOL to all languages and optimize Vue script setup traversal
  status: completed
  evidence: 'Thread-local parser pool now retains parsers for all active profile ids while preserving the specialized JavaScript, TypeScript/TSX, Python, Rust, HTML, and grammar-fallback keys. Vue script-setup fact extraction consolidates the prior repeated tree walks into one depth-first traversal and preserves callable, typed prop/iterable/slot, shadowing, reassignment, and component outputs. Verification: cargo test --test languages vue_ passed 26/26; final cargo test --test languages --quiet passed 253/253 with no ignored tests.'
- subtask_ref: align-cli-and-concurrency-docs
  title: Align docs/reference/cli.md on open_or_rebuild auto-index behavior, docs/reference/configuration.md on memory budget precedence, and update docs/architecture/concurrency.md to 20 MCP tools and modular task store paths
  status: completed
  evidence: Updated docs/reference/cli.md to distinguish missing/schema-incompatible index auto-build from existing-index source refresh, and docs/architecture/concurrency.md to describe 20 MCP tools with task-store paths under src/db/tasks_store/ and src/engine/tasks/. Also documented memory_budget_mb and FORGE_MEMORY_BUDGET_MB precedence/validation in docs/reference/configuration.md. git diff --check passed; full lib/core/languages gates pass.
receipt:
  contract_revision: 6
  passed_at: 2026-10-16T05:30:40.833555509+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib
      exit_code: 0
      tests_passed: 55
      tests_failed: 0
      log: |-
        Additional build evidence for Task2, contract revision 6, claim revision 15.
        Focused seams: cold_build_reads_source_once_and_preserves_durable_facts in src/db/writer/memory_budget_tests.rs passed 1/1 and remains a direct caller of writer::build in graph generation a52d9caec68a599902f3de977fc56077c917cf97c2c9e39f2800946941d9ea8c. public_extraction_rejects_a_file_entry_digest_mismatch in tests/core/storage_batches.rs passed 1/1.
        F1: extract_candidate performs the single cold SHA-256 and passes it to extract_typed_source_with_digest. Public and delta extraction still hashes and rejects a stale or mismatched FileEntry digest before facts publication. The public mismatch regression remains green.
        F2: mixed ordinary JS/TS script macro calls no longer poison eligible TypeScript setup slots. Consumer regression vue_typed_slot_members_follow_imported_component_provider is green in all-targets. Pre-fix actual RED is blackboard195; removing only the setup_script guard reproduced RED and was restored.
        F3: root and bounded ancestor probes apply the owning workspace ignored basenames, including linked scopes. ignored_root_ancestor_and_linked_manifests_do_not_activate_nuxt is green in all-targets. Pre-fix actual RED is blackboard196; independent root-probe and ancestor-probe mutations each reproduced consumer RED and were restored.
        F4: cli_memory_budget_precedence_fallback_and_rejection_are_observable uses child processes and proves YAML over environment, environment fallback, adaptive fallback, malformed/zero/u64-overflow rejection, and no publication on invalid settings. A YAML-selection mutation reproduced RED and was restored; broader invalid-input branch assertions are in the test.
        F5: docs/reference/configuration.md now distinguishes stable fixture/domain commitments from the global root that includes source/config fingerprinting. No fingerprint or commitment algorithm was changed.
        Registered affected gates: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib, job shell_8e63724cca39b48e, exit 0, strict Clippy clean, lib 55 passed/0 failed/0 ignored. cargo test --test core && cargo test --test languages, job shell_36f703d6548bc616, exit 0, core 33 passed/0 failed/1 ignored and languages 254 passed/0 failed/0 ignored; affected total 342/0/1.
        Full staged lifecycle gate: cargo test --all-targets, job shell_9a0f59f39b008602, exit 0, 725 passed/0 failed/1 ignored. Per-target: lib55, acdd102, cli_roots7, commitment_integrity16, core33+1 ignored, impact_context4, incremental30, languages254, linked_workspaces2, linker102, mcp78, query_context42; main binary has 0 tests.
        Release: cargo build --release job shell_14022629cb2c038a exit 0; candidate binary SHA-256 011c9f0ffdd95515bcb0164ef893cf4faa083e84258b3e6e9b9383a07bb1f877. Refreshed graph generation a52d9caec68a599902f3de977fc56077c917cf97c2c9e39f2800946941d9ea8c: 289 files, 5594 nodes, 18661 edges; direct cold-test call to writer::build verified.
        Controlled performance gate: 20 builds, job shell_a4207cb3d8eac9b3, passed. Human report benchmarks/results/048-cold-build.md; raw benchmarks/results/048-cold-build-capacity.json. Baseline binary SHA-256 86db2392333637d863f9eac4f17a3317de2b8d508a8a07a782b15b85d3543ee0; candidate SHA-256 011c9f0ffdd95515bcb0164ef893cf4faa083e84258b3e6e9b9383a07bb1f877. Wall medians 15.751s to 15.010s (-4.704%); median paired change -3.624%, satisfying both predeclared non-regression metrics. All 38818 persisted fixture Merkle rows are bit-identical with aggregate SHA-256 d98db364a389041b75c69959b2a522d590c287d83afdf3261b098fe061133e50. Graph counts, components, resolved coverage, and database size match; roots are deterministic within each binary. Cross-version output roots differ because the global root includes the source fingerprint; candidate output root a999a1e6424d855b0e5f16094ecea059bcce7db3f380c345c5184efb130b34ab. Adapter SHA-256 7a2cf281264658ca204454ca59d3ef487b4d6bffa183f12b814bb9da5a683f1e.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed frozen candidate 85ffeeb7 against accepted contract revision 6 and the original complete review. Bounded closure F1-F5 and buffer-reserve repair stay within admitted source, tests, docs and benchmarks. git diff --check passed. Reviewer made no source/docs/index/benchmark changes. External milestone031 and the approved neighboring Task4 manifest block are preserved for the coordinator; no unrelated staging or manual receipts.
        claims:
          applicable: true
          evidence: 'All F1-F5 closed with no residual: cold extract_candidate hashes its single bounded open/read once, passes the digest to the shared validator, and public/delta paths retain mismatch rejection; Vue assigned defineSlots invalidation again requires setup_script && typescript, with valid ordinary-script/TS-setup consumer positives and existing assigned/shadow negatives; root/discovered/ancestor manifest probes filter ignored basenames while retaining bounded non-symlink probes and package variants; public child-process CLI tests prove YAML over env, env fallback, adaptive omission, malformed/zero/overflow rejection without DB publication, and unchanged ordered durable domain commitments; docs distinguish facts/domain commitments from explicit YAML config fingerprinting. Checked reserve converts candidate size safely and preserves max-file, metadata, no-follow and post-read checks. Actual historical red and mutation evidence are retained honestly in .forge/tasks/048-evidence/task2-review-closure.md. Independent required gates all exit 0: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib (55 passed, 0 failed, 0 ignored; archive 946a8ee13843c4fd), cargo test --test core (33 passed, 0 failed, 1 ignored; b724e67396411244), cargo test --test languages (254 passed, 0 failed, 0 ignored; 9e933a6e3eba7804); aggregate 342 passed, 0 failed, 1 ignored. The cold one-read/durable-facts native unit seam, public wrong-digest test, ignored Nuxt consumer and typed-slot consumer ran in these complete targets. Both read-only architecture/proof helper closures and coordinator cleared this same frozen snapshot. Final retained 10+10 control series satisfies no-regression: raw wall medians 15.750766092 to 15.009530487 (-4.7060%), paired median -3.62393%; published rounded summary gives -4.704%. All 38818 domain rows/17 domains match, graph/component/DB/adapter comparisons are unchanged, and per-binary roots are stable. Independently matched candidate SHA 011c9f0ffdd95515bcb0164ef893cf4faa083e84258b3e6e9b9383a07bb1f877 and runner SHA 9f24532e68c21fc443b7b69ce0d74b7927dcb45ba491412f013c23f224c136b4 to raw provenance; failed af9e series remains disclosed. No statistical or allocation-causality claim and no cross-version global-root identity demand.'
        concurrency:
          applicable: true
          evidence: Thread-local parser pooling isolates parser state across Rayon workers, bounded source chunks retain deterministic ordered collection and one database writer, and atomic build publication remains unchanged. Checked reserve only bounds source working memory; facts/linker/SQLite are outside that scope. CLI budget proof uses child-process environment overrides rather than shared process environment. No task-store revision, transaction, lock ordering or receipt implementation changed; claim16 is distinct from builder claim15 and remains bound to snapshot85ffeeb7/baseline d4187bc.
        project_isolation:
          applicable: true
          evidence: Review and all gates ran in the exact admitted worktree. Source candidates retain workspace containment, no-follow open, regular-file and before/after metadata validation; manifest direct probes preserve checked children, ignored directories/basenames and workspace ancestor stop. Root, nested and linked-workspace ignored-manifest consumer tests pass without activating Nuxt. No linked source repositories, profiles or project namespaces were modified; benchmark artifacts describe isolated approved copies only.
        administration:
          applicable: true
          evidence: No schema, profile, protected configuration, receipt or task-administration code changed. Current accepted contract6, independent reviewer worker and frozen candidate baseline were verified from the live claim. Delivery remains same-session Forge lifecycle with atomic generated receipt and auto_commit, without manual commits, receipt edits, hooks bypasses, source edits or unrelated staging. Coordinator explicitly authorized review/pass then deliver after all three bounded closures and these complete gates pass.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository copy (commerce-release-update) must not regress; eliminating redundant file I/O and secondary filesystem traversals must reduce overall cold build latency.'
    - 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions by default, permitting granular rule enrichment without duplicating standard library tables.'
    - 'INV-FRAMEWORK-DISCOVERY: Framework manifest rules activate when explicitly placed in .forge/frameworks/ or declared in package manifests, enabling template-driven libraries like Alpine.js, HTMX, and GridViewSpec to resolve without mandatory package.json entry.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic and pass cargo test --all-targets.'
    architectural_notes:
    - 'decisions: User explicitly approved deleting only orphan records pipeline-and-core-config-decomposition and typed-node-semantics-and-module-hygiene. Pre-delete inspect confirmed each ready contract, no worker, attempts empty. Both deleted through Forge task delete --force, each count1 revoked_claims0. JSON exports previously saved /tmp/048-orphan-pipeline-task.json and /tmp/048-orphan-semantics-task.json. Scope owner conflicts for adapter.rs and vue/setup_facts.rs now cleared; extend before editing. Independent pending clarification concerns worktree rewrite/clearing and revoked build claim.'
    - 'decisions: Correction to finding188: commitments::root includes index_semantics_version, whose build.rs fingerprint hashes src/engine Rust code. Global cross-version output_root equality is therefore not a valid unchanged-fixture oracle; do not freeze/exclude version or change commitment algorithm. INV-ZERO-SEMANTIC-REGRESSION is proved with exact persisted domain commitments (all17 domains byte-equal in diagnostic DB comparison) and per-binary repeat/budget global-root determinism. Updated benchmark retains honest cross-version root equality flag and additionally hashes every ordered domain/owner_id/digest row; fails on fixture divergence or within-binary root instability. Scanner discovery intentionally omits two unused infrastructure manifests outside configured source roots; manifest_digest changes for that admitted policy change. Transient budget null serialization compatibility fixed by builder with source-unit persisted-policy test1/1; current contract6 requires fresh re-admission after external amendment, historical contract5 red remains factual evidence.'
    - 'decisions: Final benchmark oracle preserves commitments::root semantics. The global root includes engine source fingerprint and manifest discovery policy, so cross-version inequality is expected; unchanged fixture invariance is verified by exact ordered domain_commitments digests across every baseline/candidate run, with per-binary root determinism across repetitions and budgets. Diagnostic comparison covered 38818 rows in 17 domains with zero changes. Transient memory_budget_mb is excluded from persisted adapter policy, while root/ancestor discovery follows admitted revision 6 rather than restoring a second root walk. No source fingerprint or root algorithm was weakened.'
    - 'decisions: Approved final22 benchmark completed exit0; raw benchmarks/results/048-cold-build.json and readable048-cold-build.md retain exact hashes/commands. Adaptive median13.551s->13.461s(-0.664%), paired median-2.359%; modest measured improvement without significance claim. All38818 domain rows/17domains bit-identical across22; per-binary roots deterministic, candidate root1a5608d7aae3fb697d51f21e7b324feb9251e9dcfa5d1bc703f1beae1cce186d identical acrossallbudgets. Persisted adapterSHA equalsbaseline. Graph/coverage/database192741376bytes unchanged. Matrix50/200/500/1000MiB median23.210/16.785/14.895/12.825s; predefinedRSS-within5%-fastest policy selects1000MiB onthishost. Pre-policy-fix series preserved separately; it is superseded, not discarded. No furtherTask2benchmark planned absent concrete source repair.'
    - 'decisions: Before further measurements, runner now explicitly scores performance: unrounded candidate median wall time <=baseline AND median paired wall change <=0, in addition to fixture/root determinism. All runs preserved even on failure. py_compile passes. Historical repaired series exit0 was semantic-only and remains failed performance. Bounded source experiment admitted only if confirmed: reserve validated candidate.bytes before the bounded Read::take reader instead of growing an empty String; retain oneopen/oneSHA/size+metadata guards and defaultbudgetpolicy. No claimed cause or benchmark success yet; no broad unrelated optimization.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Cold build does not read source files twice from disk; scanner captures metadata only (`BuildCandidate`), while the reader reads source files once in bounded batches sized by `limits.memory_budget_mb` (falling back to `FORGE_MEMORY_BUDGET_MB` and adaptive system RAM), computing SHA-256 and extracting Tree-sitter AST facts within the same memory buffer and releasing the raw source text from RAM immediately after facts extraction.
2. `DependencyRegistry` collection eliminates the redundant `WalkBuilder` filesystem walk by consuming scanner manifest paths and performing direct root-level and ancestor manifest probes across all supported language manifest filenames (`pyproject.toml`, `package.json`, `Cargo.toml`, etc.) via `fs::read_dir`, preserving discovery for roots outside workspace root and falling back to cached paths when discovered manifests are provided.
3. Configurable RAM budget is supported via `limits.memory_budget_mb` in `forge-mcp.yaml` and `FORGE_MEMORY_BUDGET_MB` environment variable (falling back to adaptive system RAM calculation); cold build benchmarks on `commerce-release-update` across 50, 200, 500, and 1000 MB record duration vs peak RSS, verifying candidate matrix bit-identical output roots across all candidate memory budgets.
4. Parser pooling is available for all active language profiles.
5. `docs/reference/cli.md`, `docs/reference/configuration.md`, and `docs/architecture/concurrency.md` reflect verified runtime reality.
6. Cold build throughput on `commerce-release-update` is measurably improved; `cargo test --all-targets` passes with zero regressions.

---

### task: declarative-framework-packs-and-cross-repo-alignment

```yaml
task_ref: declarative-framework-packs-and-cross-repo-alignment
target: Implement framework manifest extension mode, auto-activation, GridViewSpec support, and workspace alignment
proof_policy: seam-test-first
contract_revision: 4
scope:
- src/engine/languages/manifests/framework.rs
- src/engine/languages/dependency_registry.rs
- src/engine/languages/manifests/
- docs/reference/framework-manifests.md
- docs/architecture/modularity.md
- docs/adr/0017-serde-framework-manifest-formats.md
- tests/languages/manifests.rs
- skills/contextunity-forge/references/frameworks/gridviewspec.toml.example
status: completed
subtasks:
- subtask_ref: framework-manifest-merge-and-workspace-activation
  title: 'Implement extend: true merge mode in FrameworkManifest with project rule precedence and allow activation from workspace .forge/frameworks/ directory'
  status: completed
  evidence: Default/extend merge, explicit replace, project-first identities, opaque nested values, invalid mode/schema rejection, and unconditional local activation covered by the native dependency_registry seam; framework manifest integration suite passes 254/0.
- subtask_ref: add-gridviewspec-and-rich-framework-rules
  title: Add bundled gridviewspec.toml manifest with receiver/route schema validation and auto-activation
  status: completed
  evidence: Embedded gridviewspec bundle uses the verified grid-view-spec distribution and GridViewSpec class; test proves declared activation and inactive bundled-only behavior. Canonical example now contains only verified rules.
- subtask_ref: align-connected-workspaces
  title: Align forge-mcp.yaml and .forge/ in commerce-release-update, grid-view-spec, and traverse
  status: completed
  evidence: Candidate guide validation passed on isolated commerce-release-update (3950 files), grid-view-spec (401 files), and traverse (557 files); docs record the obsolete PIM-entry prerequisite without asserting a live entry exists.
receipt:
  contract_revision: 4
  passed_at: 2026-10-16T05:58:34.532115866+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib
      exit_code: 0
      tests_passed: 56
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Full original Task3 review covered all eight admitted paths at b5b4ca9927020df80efe54be403f7fc60ed4bc21 against baseline452d0f61bfdd0feed9571d4156afd1796e7a4cb0. Closure candidate87fa621f5e36308d9fd3dd038eaf2195fe3a8652 differs only by the owning accessor comment267-270. All scoped tracked work matches candidate; gridviewspec blob3853bc1182b645adc98c2851e5f613062f2fd4d9 unchanged. No reviewer source edits; unrelated031/approvedTask4 modifications preserved.
        claims:
          applicable: true
          evidence: 'T3-DOC-1 confirmed-fixed: corrected public accessor comment accurately states unconditional owning-workspace local activation and dependency-gated bundled fallback, matching278-287. Origin incomplete-remediation; no regression test needed for comment-only maintenance. Original runtime/proof review remains valid: default/extend project-first merge, explicit replace, correct string identities, opaque values, strict arrays/unknown/mode errors, verified GridViewSpec bundle. All three stable reviewers pass bounded closure. Fresh independent cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib exit0,56passed/0failed archive0eb18860c230d687; cargo test --test languages exit0,254passed/0failed archive8ecbe277673f3b37,total310/0. Clippy contributes zero test count. Root ledger .forge/tasks/048-evidence/task3-review-closure.md.'
        concurrency:
          applicable: false
          evidence: Task3 only changes deterministic in-memory manifest parsing/merging and read-only collection. No new task transaction, claim fencing, receipt publication or process/lock behavior; closure is comment-only.
        project_isolation:
          applicable: true
          evidence: Full source review confirms local manifests keyed by workspace_path, bundled fallback still nearest-manifest dependency-gated, linked/nested isolation and scanner-supplied path/ignored-name/no-second-walk guards preserved. Native positive/negative cases and full target remain green. Original candidate guide validations passed all three isolated copies (3950/401/557 files); unchanged CLI behavior carried through comment-only closure. Originals/global agent directories untouched.
        administration:
          applicable: false
          evidence: Task3 changes no protected profiles/configuration, migrations, Git hooks, receipt machinery or candidate-parent rules. Mode flags remain skipped on serialization, conflicts fail closed. No manual receipt/status edits; same independent reviewer continues to delivery.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository copy (commerce-release-update) must not regress; eliminating redundant file I/O and secondary filesystem traversals must reduce overall cold build latency.'
    - 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions by default, permitting granular rule enrichment without duplicating standard library tables.'
    - 'INV-FRAMEWORK-DISCOVERY: Framework manifest rules activate when explicitly placed in .forge/frameworks/ or declared in package manifests, enabling template-driven libraries like Alpine.js, HTMX, and GridViewSpec to resolve without mandatory package.json entry.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic and pass cargo test --all-targets.'
    architectural_notes:
    - 'decisions: TASK_SCOPE_INVALID for reference example correctly failed outside revision3 roots; no edit/bypass occurred. Canonical Git task specification amended to revision4 with exact skills/contextunity-forge/references/frameworks/gridviewspec.toml.example scope, then supported task sync048 re-admitted it and revoked contractclaim4. This necessary co-evolution follows docs/AGENTS.md shared skill/example maintenance within existing milestone skills owner; protected frontmatter and receipts unchanged. Correct only repository example to verified distribution grid-view-spec and grid_view_spec.types.spec.GridViewSpec; no external agent directories/workspace writes. Contract author reclaims fresh revision4 before RED.'
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Project `.forge/frameworks/<id>.toml` (and `.yaml`) files extend bundled definitions with project rules taking precedence over bundled defaults, and support explicit `replace: true`.
2. Placing a manifest in `.forge/frameworks/` activates framework rules even when undeclared in `package.json` / `pyproject.toml`, reconciled with ADR 0017 and `docs/architecture/modularity.md`.
3. `gridviewspec.toml` is bundled and validates under framework manifest schema; behavioral execution of its receivers and routes is accepted in milestone 021 (task `rust-to-toml-rule-externalization`) once manifest routes/receivers are wired to extraction and linking.
4. Connected workspace adapter validation in Forge MCP verifies that configuration in `commerce-release-update`, `grid-view-spec`, and `traverse` parse cleanly, and documents prerequisites for removing obsolete `pim` references in external workspace adapters.

---

### task: handoff-landed-commit-and-shared-scope-resolution

```yaml
task_ref: handoff-landed-commit-and-shared-scope-resolution
target: Resolve verified task landing commits across authorized later shared-scope changes and scope-equivalent repacks without manual receipt edits
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/engine/milestones/handoff.rs
- tests/acdd/milestones.rs
- docs/reference/cli.md
status: completed
subtasks:
- subtask_ref: resolve-verified-landings-and-shared-scope
  title: Resolve actual landed commits from accepted task delivery provenance, including scope-equivalent repacks, and validate later shared-file changes against completed task evidence
  status: completed
  evidence: Frozen candidate3109b9c reviewed by three stable independent reviewers. Own first-parent landing and scope-equivalent repack resolution accepts legitimate reviewed shared evolution, with accepted gate/closed claim provenance, configured independence, reviewed candidate to delivery scoped content, and final shared-value authority verified. T4-PROV-1/-2 closed; CLI record_task_commits behavior documented.
- subtask_ref: prove-handoff-provenance-and-atomic-failure
  title: Prove shared-scope and repack handoff acceptance plus fail-closed rejection and non-mutation through the existing real Git, task lifecycle, SQLite, and archive seam
  status: completed
  evidence: Independent strict Clippy clean; complete cargo test --lib57/0 and cargo test --test acdd103/0. Expanded real Git/tasks/SQLite/archive16-case seam passes1/0 after six causal RED regressions (archive052c21a5e67442dc). Retained independent CLI fixture control passes, all six provenance/baseline tamper cases reject with active doc preserved/no archive. Same three reviewers bounded closurePASS for T4-PROV-1/-2 and T4-DOC-1.
receipt:
  contract_revision: 2
  passed_at: 2026-10-16T07:03:51.077385364+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 103
      tests_failed: 0
      log: 'Final required scoped commands also passed: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib (57 passed, 0 failed); cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd (103 passed, 0 failed). Focused 16-case handoff seam passed (1 test, 0 failed, 102 filtered).'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed frozen scoped candidate3109b9c168e09fdc592b12cf7476c891ad8e4b21 and bounded repair against rejected18ce984. All Task4 changes remain in handoff.rs, tests/acdd/milestones.rs and docs/reference/cli.md; scoped candidate equality and git diff --check pass. Unrelated milestone031 and existing Rust formatting paths preserved. Reviewer made no source/docs/test/receipt edits.
        claims:
          applicable: true
          evidence: 'All three original review contours completed, then the same primary, architecture and proof reviewers independently passed bounded closure of exactly T4-PROV-1, T4-PROV-2 and T4-DOC-1. Unique persisted closed-claim worker/worktree binding and configured independent_from enforcement close PROV-1; accepted delivery scoped content is checked against reviewed snapshot excluding only the actual active milestone receipt path, with review/delivery baseline binding, closing PROV-2. CLI now distinguishes conditional Markdown SHA from SQLite actual landing SHA, closing DOC-1. Original shared/repack/no-op/first-parent, rejection, omission and rollback behavior retained. Independent exact commands: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib; cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd. Both exit0, strict Clippy clean; lib57/0 and acdd103/0, total160/0, no ignored or filtered tests. Focused expanded16-case seam passes1/0. Retained independent disposable CLI fixture script now accepts control and rejects all six tamper variants with active Markdown preserved and no archive.'
        concurrency:
          applicable: true
          evidence: All task provenance and shared-scope validation occurs before archive creation and SQLite receipt mutation. Existing rollback compensation and expected task-set transaction checks are retained and covered by complete103-test ACDD gate. New claim reads require a unique ended attempt and current gate envelope consistency; synchronous local provenance behavior adds no storage schema, locking or concurrency contract.
        project_isolation:
          applicable: true
          evidence: Exact milestone worktree and worktree-bound Forge graph generation ae6f2bba830938409167789cc7ab0ac8ea15659f3bcf8c7a8ed25aa9fbaa698d verified. Provenance binds evidence worktree to its persisted attempt; mismatching worktree fixture now fails. All independent fault injection occurred in disposable /tmp Git/SQLite repositories using the current candidate target/debug binary. Canonical active milestone receipt path is the sole scoped-content comparison exemption.
        administration:
          applicable: true
          evidence: Accepted gate proofs, current contract revisions, commit objects, snapshot baselines and configured independent workers are validated before landing authority is used. No schema/profile or core task commit policy changed. Auto delivery, repack/no-op landing resolution, actual own SHA recording, record_task_commits:false Markdown omission and rollback gates pass. No reviewer manual commit, receipt edit, handoff, release or benchmark. Root consolidated all three same-reviewer closure passes and authorized this review pass plus automatic delivery.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository copy (commerce-release-update) must not regress; eliminating redundant file I/O and secondary filesystem traversals must reduce overall cold build latency.'
    - 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions by default, permitting granular rule enrichment without duplicating standard library tables.'
    - 'INV-FRAMEWORK-DISCOVERY: Framework manifest rules activate when explicitly placed in .forge/frameworks/ or declared in package manifests, enabling template-driven libraries like Alpine.js, HTMX, and GridViewSpec to resolve without mandatory package.json entry.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic and pass cargo test --all-targets.'
    architectural_notes:
    - 'decisions: User explicitly approved this fourth milestone048 task after tasks2 and3, before final handoff. Production ownership is only src/engine/milestones/handoff.rs; tests/acdd/milestones.rs is admitted solely for the existing real Git/task lifecycle/SQLite/archive verification seam. Do not edit receipts or SQLite manually, do not trust scope overlap or commit messages as proof, and preserve fail-closed rejection and archive non-mutation. No implementation starts before tasks2 and3 are delivered.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Handoff requires every milestone task to be completed with valid current-contract delivery and review receipts. Accepted task evidence establishes trust; commit messages and scope membership alone do not.
2. A later completed and reviewed task can change an earlier task's shared scoped file. Resolve each task's own landing against its accepted delivery and validate the later accepted change without requiring the earlier task's full scoped tree to equal the final tip.
3. Resolve scope-equivalent repacks from real milestone history and original accepted provenance. Handoff records actual landed SHAs in SQLite and Markdown according to `record_task_commits`, without manual receipt or SQLite edits.
4. Reject final shared-path changes without accepted completed-task evidence, malformed or missing landings, unrelated or obsolete landings, non-equivalent repacks, and invalid or tampered receipt proof. Declaring an overlapping scope does not authorize an unreviewed change.
5. Rejected handoff preserves active Markdown and SQLite receipts and creates no archived milestone. Successful handoff preserves archive and receipt atomicity and rollback behavior.
6. `docs/reference/cli.md` accurately documents milestone handoff landed commit resolution and shared scope verification behavior.
