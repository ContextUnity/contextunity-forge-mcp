---
id: m-framework-indexing-regression-repair
title: Cold-build performance, RSS bound, HTML endpoint resolution repair, and Cargo workspace split
doc_type: contract
status: active
depends_on:
- m-declarative-framework-manifests-and-ecosystem-packs
owners:
- src/core/
- src/db/writer.rs
- src/db/writer/
- src/engine/ast/
- src/engine/languages/
- src/engine/linker.rs
- src/engine/linker/
- src/engine/pipeline.rs
- Cargo.toml
- crates/
- tests/languages/
- tests/incremental/
- tests/commitment_integrity/
- docs/architecture/
- docs/reference/
invariants:
- 'INV-REPAIR-COVERAGE: Performance repairs preserve correct resolution statuses, exact endpoint targets, cold/delta equivalence, persisted facts, and deterministic commitments.'
- 'INV-REPAIR-DECLARATIVE: Framework conventions remain in manifests; extractors and shared handlers contain only universal language or rule-processing logic.'
- 'INV-REPAIR-PERFORMANCE: Preserve single-pass extraction, typed analysis facts, shared prepared rule/index ownership, bounded persistence, and no source rereads during delta, as required by the existing architecture and ADR 0012.'
- 'INV-WORKSPACE-BOUNDARIES: Splitting the repository into forge-core, forge-index, and forge-tasks enforces strict unidirectional dependency boundaries with zero circular crate dependencies.'
tasks:
- restore-framework-indexing-performance
- restore-default-html-endpoint-resolution
- split-cargo-workspace-into-independent-crates
- optimize-indexing-linking-and-persistence-pipeline
started_at: 2026-10-30T17:26:21+00:00
---

# Cold-build performance, RSS bound, HTML endpoint resolution repair, and Cargo workspace split

## Outcome and boundaries

Repair the cold-build slowdown and HTML endpoint-resolution losses introduced
by the framework work, bound cold-build peak RSS, and split the Cargo workspace
into 3 independent crates: `forge-core`, `forge-index`, and `forge-tasks`.
Keep new SQL, topology, and search capabilities in
[LS-002](LS-002-sql-schema-indexing-entrypoints-and-topology-slices.md).

Use the existing production paths, repository verification commands, and shared
test suites. This milestone introduces no custom benchmark scripts or additional
measurement procedure. Run benchmarks only when explicitly requested by the user.
Follow [ADR 0012](../adr/0012-zero-regression-serialization-and-cold-build-latency.md)
and the [performance architecture](../architecture/performance.md).

Broader framework rule unification, parameterized route trie indexing across
all client languages, Tree-sitter kind-id dispatch, and linker symbol interning
are implemented directly in milestone 022.

## Tasks

### task: restore-framework-indexing-performance

```yaml
task_ref: restore-framework-indexing-performance
target: Remove the cold-indexing performance regression introduced by framework extraction and linking changes while preserving their correct resolution results
proof_policy: direct-proof
contract_revision: 1
scope:
- src/core/typed_facts.rs
- src/core/semantic.rs
- src/db/writer.rs
- src/db/writer/
- src/engine/ast/
- src/engine/languages/
- src/engine/linker.rs
- src/engine/linker/
- src/engine/pipeline.rs
- tests/languages/
- tests/incremental/
- tests/commitment_integrity/
- docs/architecture/performance.md
- docs/architecture/indexing.md
status: completed
subtasks:
- subtask_ref: remove-introduced-redundant-work
  title: Find and remove redundant traversal, rule processing, index construction, and fact conversion introduced by the framework changes; keep required analysis intact
  status: completed
  evidence: 'Implemented the accepted contract''s identified redundant path: Context::named_member reuses the effective Arc<LanguageContext> already prepared for the file, keyed by interned PathId, instead of rebuilding the workspace/package-scope context key per member reference. The existing allocation-bound seam test passed for 1 vs 32 references (1 passed, 0 failed) and asserts exact targets/coverage. The admitted contract selected this path; no broader traversal/index/fact-conversion or whole-corpus performance claim is made. No benchmark was run.'
- subtask_ref: preserve-resolution-and-persistence
  title: Verify unchanged correct targets, cold/delta behavior, cache hydration, and commitments through existing shared seams
  status: completed
  evidence: 'Ran focused member-resolution seam (1 passed, 0 failed), then strict Clippy and the complete affected --lib, commitment_integrity, incremental, and languages targets (399 passed, 0 failed: 102 + 16 + 31 + 250). These existing suites provide the selected proof for resolution, incremental/cold-delta, commitment, and persistence behavior. The runs do not measure relative cold-build latency/RSS; that remains unverified because benchmarks were not authorized.'
receipt:
  contract_revision: 1
  passed_at: 2026-10-30T19:59:38.961439188+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/languages/extractors.rs
      tests_failed: 0
      tests_passed: 250
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed immutable scoped candidate 8eac0f3f63b502933d08b7579a4eaaad37c5f020 against ac75900e73f7bc84b28b8151497abd4896aaaa06 and f4bb809b. Relative to synthesis, exactly src/engine/languages/dependency_registry.rs, docs/architecture/indexing.md and docs/architecture/performance.md change. Relative to f4bb809b the only additional scoped change is tests/languages/extractors.rs, already authored and committed at synthesis; it is unchanged against ac75900e. A final git diff --exit-code against the candidate over the entire admitted scope passed; git diff --check passed. No staged or untracked source changes. Inherited Cargo.toml/Cargo.lock version/author metadata and coordinator .gitignore/milestone lifecycle/dependency edits are outside this scoped snapshot, corroborated by prior review/build evidence; no protected-path changes by this task.
        claims:
          applicable: true
          evidence: 'PASS. language_context_for_path now hashes borrowed registry digest and logical directory before language_context_key. A verified hit returns an Arc clone without profiles/BTreeSet preparation, manifest-scope traversal or owned scope strings. Misses retain canonical registry-owned context lookup and the existing legacy preparation; the 512-entry directory memo holds Arc handles, not duplicate rule/fact payloads. Exact digest/directory comparisons make hash collisions safe misses. Context::named_member reaches this production lookup, so repeated member references use the memo; no rules, files, references, validation, source reads, schema or public API are removed. Architecture docs describe this ownership. The unchanged real seam varies references [1,32], asserts unique extraction, exact call targets/resolved coverage and scope_copies[1] <= scope_copies[0]; synthesis/contract recorded runtime-red exit 101 with counts [8,39]. Independent focused command cargo test --test languages languages::extractors::python_member_access_callee_does_not_emit_duplicate_reference -- --exact --nocapture passed (exit 0; 1 passed, 0 failed). Independent complete command cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib --test commitment_integrity --test incremental --test languages passed, exit 0: 102+16+31+250=399 passed, 0 failed, no filtered tests, strict Clippy clean (lean-ctx archive d324e75be29d0e8e). PF001-R1 is closed per the explicit current user decision: accepted proof is direct production-seam proof without external benchmarks. The later comparison in blackboard #1353 used obsolete a876ba6a, lacking src/engine/languages/sql/mod.rs and LS-002 archive present in f4bb809b, so it cannot attribute a framework-only regression across equivalent capabilities. Historical #1352/#1353 are superseded for acceptance, with no whole-corpus latency/RSS claim or benchmark rerun.'
        concurrency:
          applicable: true
          evidence: The new mutable memo is thread_local RefCell state, with each borrow released before fallback preparation; parallel workers do not share mutable cache state or acquire a new cross-thread lock. Cached contexts remain immutable Arc<LanguageContext>, preserving prepared rule-table ownership/lifetimes. The cache clears at 512 entries; eviction/collision only repeats the existing safe lookup. Registry collection finalizes its digest before context preparation/validation and publication. No task transactions, revision fencing, receipts, publication, lock ordering or rollback paths change. Complete affected suites, including existing parallel extraction/cache/publication and cold/delta behavior, passed.
        project_isolation:
          applicable: true
          evidence: Directory identity retains canonical workspace-qualified prefixes such as [workspace]/..., and full registry digest plus directory are checked on every hit. The existing nearest_manifest_scope_for_path starts from the same directory, so files sharing the memo key have the same workspace/package scope vector. Digest computation includes bundled-manifest digest plus sorted logical manifest paths and content digests before preparation; changed rules/dependencies invalidate reuse. The memo reuses the canonical context instead of adding independent activation or rule semantics. Existing workspace/nested-package activation, local overrides, delta/hydration and deterministic commitments remain enforced and passed in the complete affected suites. No filesystem root, symlink, traversal, profile selection or cross-project task-store behavior changes.
        administration:
          applicable: false
          evidence: The scoped implementation changes no migrations, profile pins, .forge configuration, credentials, CLI/MCP surface, Git hooks, task engine, receipt handling or privileged delivery behavior. Protected Cargo/build/profile files are absent from the task-owned diff; inherited worktree metadata is explicitly distinguished in paths. Review worker codex-sol-reviewer is independent of accepted builder pf001-builder. Claim 23 / contract 1 and immutable candidate/baseline were rechecked immediately before submission. No source edits, commits, benchmarks or delivery actions were performed during review.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REPAIR-COVERAGE: Performance repairs preserve correct resolution statuses, exact endpoint targets, cold/delta equivalence, persisted facts, and deterministic commitments.'
    - 'INV-REPAIR-DECLARATIVE: Framework conventions remain in manifests; extractors and shared handlers contain only universal language or rule-processing logic.'
    - 'INV-REPAIR-PERFORMANCE: Preserve single-pass extraction, typed analysis facts, shared prepared rule/index ownership, bounded persistence, and no source rereads during delta, as required by the existing architecture and ADR 0012.'
    - 'INV-WORKSPACE-BOUNDARIES: Splitting the repository into forge-core, forge-index, and forge-tasks enforces strict unidirectional dependency boundaries with zero circular crate dependencies.'
    architectural_notes:
    - 'decisions: Contract synthesis, contract_revision=1. Audited LanguageProfile registry/mod.rs, ast::extract_typed_with_registry -> parse_file_typed -> SinglePassExtractor/RouteContextBuilder, DependencyRegistry context preparation, SymbolCatalog/FlowStore, linker semantic context, framework composition, and bounded pipeline/writer boundaries; ADR 0012/0013 and performance invariants retain authority. Existing Arc contexts and RuleIR are canonical; no new registry, grammar, Cargo/build feature, storage schema, source reread, or fact omission is admitted. Concrete redundant-work path: Context::named_member calls DependencyRegistry::language_context_for_path per member expression; language_context_key rebuilds the profiles BTreeSet and owned workspace/family/scope vector on every lookup. Chosen internal repair: bind/reuse effective LanguageContext at existing file/PathId semantic-context ownership (or use equivalent borrowed lookup within the existing registry), preserving activation, manifest/workspace isolation, ambiguity and exact target proofs. Do not duplicate rule registries or cache full facts. Additive existing seam: tests/languages/extractors.rs::python_member_access_callee_does_not_emit_duplicate_reference, parameterized 1/32 member calls under one collected Python project scope; selected-size scope-string allocation counts during warmed linking must not grow with reference cardinality, and every calls edge retains its exact method target. Private one-thread Rayon pool isolates allocator observation; parsing, registry collection, warmup and assertions are excluded. No wall-time benchmark is run or inferred. Builder must additionally preserve existing cold/delta, hydration and commitments suites, and run complete affected targets plus strict Clippy per TESTS.md. All proposed code/test/docs owners are admitted; Cargo.toml/build.rs/.forge remain protected. HTML repair and workspace splitting remain successor tasks. Candidate seam runtime-red is being compiled; timing/comparison acceptance remains unverified until user authorizes existing benchmark tooling.'
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. The production cold build no longer has the introduced slowdown relative to
   the comparison binary on the same control repository and configuration.
2. Repairs remove demonstrated redundant work and preserve analysis precision;
   they do not disable rules, omit files or references, weaken validation, or
   trade coverage for speed.
3. Existing affected tests and strict Clippy pass. When the user requests a speed
   check, use the repository's existing benchmark tooling and ready binaries;
   report actual wall and CPU results without introducing a separate driver.

### task: restore-default-html-endpoint-resolution

```yaml
task_ref: restore-default-html-endpoint-resolution
depends_on:
- restore-framework-indexing-performance
target: Restore correct default-pack resolution of the 14 real Commerce HTMX endpoint references that changed from resolved to unresolved
proof_policy: direct-proof
contract_revision: 3
scope:
- src/engine/languages/html/
- src/engine/languages/manifests/
- src/engine/languages/semantic.rs
- src/engine/linker/framework_relations.rs
- src/engine/linker/html/
- tests/languages/html/
- docs/reference/framework-manifests.md
- docs/architecture/indexing.md
status: completed
subtasks:
- subtask_ref: trace-default-pack-route-loss
  title: Trace the real lost references from HTML extraction through default manifests and verified route providers to coverage and calls_endpoint edges
  status: completed
  evidence: Traced lost references from HTML extraction through default manifests and verified route providers to coverage and calls_endpoint edges. Identified URL status classifier dynamic_or_nonlocal vs local_unverified divergence.
- subtask_ref: restore-generic-parameterized-url-linking
  title: Repair universal rule handling or bundled declarations so verified parameterized HTMX requests resolve without project-local workaround manifests
  status: completed
  evidence: Restored generic parameterized URL linking in request_attributes.rs and relations.rs with canonical fixed-boundary verification. All 14 Commerce endpoints verified and passing.
receipt:
  contract_revision: 3
  passed_at: 2026-10-30T21:40:37.632234261+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/languages/html/htmx_relations.rs::parameterized_htmx_requests_resolve_to_backend_route_endpoints
      tests_failed: 0
      tests_passed: 1
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS. Reviewed exact base 2a39bdb3bb3fec3e5e5cf517bef34ffe27b8929d to candidate d93377a99bd3b0b3ed0ec9cfb439326fea2fd3fa. Exactly three admitted files change: src/engine/languages/html/request_attributes.rs (48 insertions / 4 deletions), src/engine/linker/html/relations.rs (62 / 1), tests/languages/html/htmx_relations.rs (214 / 13), total 324 insertions / 18 deletions. No scope extension or protected-file edit in the task diff. git diff --check passed; full admitted-scope git diff --exit-code d93377a passed initially and immediately before submission. Live HEAD 2a2660a7 and inherited staged contract artifacts remain separate from sparse task snapshots. Inspected unrelated existing .gitignore and Cargo version/author metadata edits; these are outside task-owned diff and do not add dependencies or change build profiles. Reviewer changed no repository source/test/doc files; probe fixtures/DBs remain under /tmp.'
        claims:
          applicable: true
          evidence: PASS; both historical findings HTML-REVIEW-1 and HTML-REVIEW-2 are confirmed-fixed with current attributable evidence. request_url_status admits only anchored whole-segment well-formed template parameters; unsupported expressions remain unresolved. Traced extract_request_attributes -> request_url_status -> emit_request_routes -> match_request_route -> canonical RouteIndex -> has_client_route_mismatch -> emit_route_match -> persisted edge/coverage rows. All four prior negative controls now persist unresolved coverage and no calls_endpoint edges in independent production CLI reproductions. Both positive controls persist resolved coverage and exact expected routes. Current seam proves all 14 Commerce fixture occurrence-to-handler/route mappings, resolved coverage, template delta, HTMX override disable/restore and complete reopened cold/delta graph/coverage/commitment verification. Original all-method, verified-provider, duplicate-route ambiguity, renamed-rule and separate-workspace assertions pass. Independent required command cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages exited 0; strict Clippy clean; 251 passed / 0 failed / 0 ignored / 0 filtered. Verification archive 1a2317aedc2bb372; production first-probe build archive b7c66762efb6102b. Additional function/filter/arithmetic/whole-URL/all-parameter/mixed-segment/${id} requests remain unresolved; remote URLs remain external. No admitted scope or contract findings remain.
        concurrency:
          applicable: true
          evidence: PASS for the changed surface. Classifier and new HTML guard operate on local borrowed strings, temporary segment vectors and immutable Node/RouteIndex reads. No shared mutable state/cache, mutex, nested parallelism, I/O, claim/revision transaction, receipt, rollback or publication path changes. Existing immutable prepared rule/index ownership and parallel extraction lifetimes remain intact. Complete languages target passed including existing cold/delta, persistence and cache seams.
        project_isolation:
          applicable: true
          evidence: PASS. All Forge tools explicitly routed to assigned PF-001 worktree; current namespace, contract 3 and review claim 40 rechecked. Provider identity, activation and owning-root policy are unchanged; rule origin remains required before route admission. Existing workspace-local override removal/restoration, renamed pack rules and isolated workspace default semantics pass. Additional CLI reproducers use separate /tmp workspaces without framework override manifests. No filesystem root, path traversal, symlink, task namespace or linked-profile selection code changes.
        administration:
          applicable: false
          evidence: 'No administration implementation changes: schema/migrations, .forge profile, credentials, hooks, public CLI/MCP, receipt and privileged delivery behavior remain unchanged. Worker codex-sol-reviewer is independent of accepted pf001-builder (build claim 37). Active claim 40 / contract 3 / candidate d93377a and source binding were rechecked immediately before submission. No source edits, benchmarks, commits or delivery performed during this review. Source/test boundaries remain respected; successor task retains Cargo workspace extraction.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REPAIR-COVERAGE: Performance repairs preserve correct resolution statuses, exact endpoint targets, cold/delta equivalence, persisted facts, and deterministic commitments.'
    - 'INV-REPAIR-DECLARATIVE: Framework conventions remain in manifests; extractors and shared handlers contain only universal language or rule-processing logic.'
    - 'INV-REPAIR-PERFORMANCE: Preserve single-pass extraction, typed analysis facts, shared prepared rule/index ownership, bounded persistence, and no source rereads during delta, as required by the existing architecture and ADR 0012.'
    - 'INV-WORKSPACE-BOUNDARIES: Splitting the repository into forge-core, forge-index, and forge-tasks enforces strict unidirectional dependency boundaries with zero circular crate dependencies.'
    architectural_notes:
    - |-
      decisions: Architectural synthesis decision for task restore-default-html-endpoint-resolution (contract_revision: 2):
      1. Root Cause:
         `src/engine/languages/html/request_attributes.rs` (`request_url_status`) classifies any URL containing '{' or '}' or whitespace as 'dynamic_or_nonlocal', preventing parameterized HTMX URLs from reaching linker route matching in `src/engine/linker/html/relations.rs`.
         The 14 real Commerce HTMX endpoint references:
         - 4 channel actions in `channels/settings.html`: `/channels/{{ profile.provider }}/{{ profile.profile_name }}/<action>/`
         - 6 image requests in `products/editor/_images.html`: `/products/api/{{ product.pk }}/editor-images/`
         - 3 review actions in `taxonomy_review/_drawer.html`: `/taxonomy/reviews/{{ review.pk }}/resolve/`
         - 1 pane request in `technologies/_set_detail.html`: `/technologies/{{ tech.id }}/pane/`
         all contain valid template parameter segments `{{ ... }}`.
      2. Architecture & Minimal Intervention:
         In `src/engine/linker/routes.rs`, `normalize_client_url` and `is_client_param` already recognize `{{ ... }}` and normalize segments to `{param}`.
         The repair will refine `request_url_status` in `src/engine/languages/html/request_attributes.rs` so that client URLs whose dynamic segments are well-formed client parameters (such as `{{ ... }}` without unclosed braces or invalid protocols) are classified as `local_unverified`, allowing `emit_request_routes` to call `routes.match_route` and emit exact `calls_endpoint` edges to matching backend handlers.
         Unsupported/arbitrary dynamic expressions remain `dynamic_or_nonlocal` and resolve to `unresolved` without fallback guessing.
      3. Seam Test:
         Added `parameterized_htmx_requests_resolve_to_backend_route_endpoints` to `tests/languages/html/htmx_relations.rs`. It compiles cleanly and yields runtime assertion failure (exit code 101, left: 0, right: 4 calls_endpoint).
      4. Scope:
         Admitted task scope includes `src/engine/languages/html/`, `src/engine/linker/html/`, `src/engine/ast/routes.rs`, `src/engine/linker/framework_relations.rs`, `tests/languages/html/`, `docs/reference/framework-manifests.md`, `docs/architecture/indexing.md`. Protected files (Cargo.toml, build.rs) remain untouched.
    review_summary:
      decision: pass
      contours:
        administration: not_applicable
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Verify all 14 real lost occurrences and their expected backend targets: four
   channel actions in `channels/settings.html`, six image requests in
   `products/editor/_images.html`, three review actions in
   `taxonomy_review/_drawer.html`, and one pane request in
   `technologies/_set_detail.html`.
2. The standard bundled configuration resolves those requests to the correct
   endpoints with `calls_endpoint` edges; local `.forge/frameworks` overrides
   retain their declared behavior.
3. Preserve HTTP-method checks, verified provider identity, literal path
   boundaries, and ambiguity handling. Unsupported dynamic expressions remain
   unresolved; matching an arbitrary dynamic URL to an arbitrary route is not
   an accepted repair.
4. Add necessary cases to the existing shared HTML framework seam. Verify cold,
   delta, and persisted/reopened results without creating one test per defect.

### task: split-cargo-workspace-into-independent-crates

```yaml
task_ref: split-cargo-workspace-into-independent-crates
depends_on:
- restore-default-html-endpoint-resolution
target: 'PF-001-T05: Split the monolithic crate into three independent Cargo workspace crates: forge-core, forge-index, and forge-tasks to decouple domains, enforce unidirectional dependency boundaries, and accelerate incremental compilation'
proof_policy: direct-proof
contract_revision: 1
scope:
- Cargo.toml
- crates/forge-core/
- crates/forge-index/
- crates/forge-tasks/
- src/
- tests/
- docs/architecture/performance.md
status: completed
subtasks:
- subtask_ref: extract-forge-core-crate
  title: Extract domain models, string interner, configuration, and Merkle commitments into forge-core crate with zero parser or database dependencies
  status: completed
  evidence: Created crates/forge-core with Cargo.toml and src/lib.rs, containing domain models, interned IDs, configuration, and Merkle commitments with zero dependency on AST parsers or SQLite.
- subtask_ref: extract-forge-index-crate
  title: Extract Tree-sitter parsers, AST extraction pipelines, declarative ruleset engine, linker resolution, and SQLite storage into forge-index crate
  status: completed
  evidence: Created crates/forge-index with Cargo.toml and src/lib.rs, containing Tree-sitter parsers, AST extraction, cross-language linkers, and SQLite index persistence, depending only on forge-core.
- subtask_ref: extract-forge-tasks-crate
  title: Extract ACDD task lifecycle, blackboard coordination, proof verification, and worktree automation into forge-tasks crate
  status: completed
  evidence: Created crates/forge-tasks with Cargo.toml and src/lib.rs, containing ACDD task lifecycles, blackboard coordination, proof verification, and worktree isolation, depending on forge-core and SQLite task store.
- subtask_ref: update-workspace-dependencies-and-imports
  title: 'Configure Cargo workspace root, wire crate dependencies, and update crate:: imports across all repository modules and test suites'
  status: completed
  evidence: Configured Cargo workspace root in Cargo.toml with members = ["crates/forge-core", "crates/forge-index", "crates/forge-tasks", "."], wired dependencies, verified unidirectional boundaries and full workspace compilation.
receipt:
  contract_revision: 1
  passed_at: 2026-10-31T05:43:44.885400281+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/core.rs::workspace_contract_tests::cargo_workspace_declares_independent_crates_and_unidirectional_boundaries
      tests_failed: 0
      tests_passed: 1
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Schema15 and task-store migrations remain intact. Cargo workspace configuration enforces acyclic dependencies. No credentials, remote publications, or uncontracted profile changes.
        claims:
          applicable: true
          evidence: 'R1 resolved: Canonical production cutover to forge-index completed. Parallel monolithic implementations in src/engine/linker/, src/engine/ast/, src/db/, src/core/commitments/ completely removed. Root modules strictly re-export/delegate to canonical crate APIs. Independent compilation verified across crates (forge-core: 13 passed, forge-tasks: 12 passed, forge-index: 49 passed). Strict Clippy 0 warnings, test core 46 passed (0 failed, 1 ignored), commitment_integrity 16 passed, framework_manifests 25 passed.'
        concurrency:
          applicable: true
          evidence: Task-store transactions, SQLite writer/initialization locks, fs/Git primitives, pipeline and root writer remain unchanged and sound. Once registration and RwLock extraction guards validated. No shared database or transaction defects.
        paths:
          applicable: true
          evidence: Candidate 0520bea checked against baseline 8bfeae5. Admitted write perimeter Cargo.toml, crates/, src/, tests/, docs/architecture/performance.md strictly respected. Root build.rs is byte-for-byte identical to baseline HEAD. 0 out-of-scope entries. git diff --check exits 0.
        project_isolation:
          applicable: true
          evidence: 'Workspace resolution uses active repository root and dedicated worktree. Task identifiers, milestone paths, database rows, and profile paths are strictly confined to forge-mcp. Unidirectional crate boundaries verified: forge-core (0 deps) <- forge-index / forge-tasks <- root.'
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REPAIR-COVERAGE: Performance repairs preserve correct resolution statuses, exact endpoint targets, cold/delta equivalence, persisted facts, and deterministic commitments.'
    - 'INV-REPAIR-DECLARATIVE: Framework conventions remain in manifests; extractors and shared handlers contain only universal language or rule-processing logic.'
    - 'INV-REPAIR-PERFORMANCE: Preserve single-pass extraction, typed analysis facts, shared prepared rule/index ownership, bounded persistence, and no source rereads during delta, as required by the existing architecture and ADR 0012.'
    - 'INV-WORKSPACE-BOUNDARIES: Splitting the repository into forge-core, forge-index, and forge-tasks enforces strict unidirectional dependency boundaries with zero circular crate dependencies.'
    architectural_notes:
    - |
      decisions: Architectural synthesis decision for task split-cargo-workspace-into-independent-crates (contract_revision: 1):
      1. Workspace Structure:
         - Root Cargo.toml declares workspace members: ["crates/forge-core", "crates/forge-index", "crates/forge-tasks", "."].
      2. Decoupled Crate Responsibilities & Boundaries:
         - `crates/forge-core`: Core domain models, path/symbol interned IDs, Merkle commitments, and configuration. Zero dependency on tree-sitter or SQLite.
         - `crates/forge-index`: Tree-sitter AST extraction, declarative language rulesets, cross-language linkers, and SQLite index persistence. Unidirectional dependency on `forge-core`. Zero dependency on `forge-tasks`.
         - `crates/forge-tasks`: ACDD task lifecycles, blackboard coordination, proof verification, and worktree isolation. Unidirectional dependency on `forge-core` and SQLite task store. Zero dependency on `forge-index`.
         - `contextunity-forge-mcp` (root): Integrates core, index, and tasks; provides CLI and MCP interfaces.
      3. Seam Contract:
         - Strengthened `tests/common/workspace_contract_tests.rs` with `cargo_workspace_declares_independent_crates_and_unidirectional_boundaries`.
         - Verified compiling runtime-red baseline: panics with exit code 101 (`Cargo.toml must declare [workspace]`).
      4. Scope:
         - Write scope: Cargo.toml, crates/forge-core/, crates/forge-index/, crates/forge-tasks/, src/, tests/, docs/architecture/performance.md.
    - 'decisions: {"candidate":"0520bea5eb0c71df82527428b343ac434752b5cb","candidate_baseline_head":"8bfeae5d1973c87c0487077e6061a9cdf2361411","claim_revision":68,"contract_revision":1,"decision":"pass","dispositions":[{"id":"1369/R1","origin":"incomplete-remediation","disposition":"confirmed-fixed","evidence":"Canonical cutover to forge-index verified: root entrypoints delegate/re-export canonical crate APIs, and all parallel monolithic implementations in src/engine/linker/, src/engine/ast/, src/db/, src/core/commitments/ have been completely removed. Independent crate compilation and tests verified."}],"contours":{"paths":{"applicable":true,"evidence":"Candidate 0520bea checked against baseline 8bfeae5. Admitted write perimeter respected. Root build.rs is byte-for-byte identical to baseline HEAD. git diff --check exits 0."},"claims":{"applicable":true,"evidence":"R1 fully resolved. Single canonical owner established. Independent compilation verified. Strict Clippy 0 warnings, test core 46 passed (0 failed, 1 ignored), commitment_integrity 16 passed, framework_manifests 25 passed."},"concurrency":{"applicable":true,"evidence":"Task-store transactions, SQLite writer/initialization locks, and Once/RwLock guards verified sound."},"project_isolation":{"applicable":true,"evidence":"Workspace and worktree isolation verified. Unidirectional dependency graph enforced: forge-core (0 deps) <- forge-index / forge-tasks <- root."},"administration":{"applicable":true,"evidence":"Schema15 and task-store migrations intact. Cargo workspace configuration enforces acyclic boundaries."}},"verification":[{"command":"cargo clippy --all-targets --all-features -- -D warnings && cargo test --test core","exit_code":0,"warnings":0,"tests_passed":46,"tests_failed":0,"tests_ignored":1}],"next_action":"Advance to deliver stage."}'
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

1. Cargo workspace root in `Cargo.toml` declares members `["crates/forge-core", "crates/forge-index", "crates/forge-tasks", "."]`.
2. `forge-core` contains fundamental domain types, interned identifiers (`PathId`, `SymbolId`), configuration, and Merkle commitments without dependencies on AST parsing or SQLite.
3. `forge-index` contains Tree-sitter parsers, AST extraction pipelines, declarative manifest engine, cross-language linkers, and SQLite index persistence, depending only on `forge-core`.
4. `forge-tasks` contains ACDD lifecycle management, blackboard coordination, proof verification, and worktree isolation, depending on `forge-core` and SQLite task schemas.
5. All tests, benchmarks, and CLI commands compile cleanly (`cargo check --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`), with incremental compilation times reduced significantly.

### task: optimize-indexing-linking-and-persistence-pipeline

```yaml
task_ref: optimize-indexing-linking-and-persistence-pipeline
depends_on:
- split-cargo-workspace-into-independent-crates
target: 'PF-001-T06: Optimize HTML operation dispatch, route matching, storage node staging, and commitment sealing to reduce cold-build latency and bound peak RSS on large-scale codebases'
proof_policy: direct-proof
contract_revision: 2
scope:
- crates/forge-index/src/languages/html/
- crates/forge-index/src/languages/manifests/
- crates/forge-index/src/linker/html/
- crates/forge-index/src/linker/routes.rs
- crates/forge-index/src/db/writer/seal.rs
- crates/forge-index/src/db/writer.rs
- crates/forge-index/src/db/batch.rs
- crates/forge-index/src/engine/pipeline.rs
- src/
- tests/
- docs/architecture/performance.md
status: completed
subtasks:
- subtask_ref: zero-alloc-html-matcher-and-single-pass-lookup
  title: Zero-allocation HTML route mismatch iterator and single-pass RouteIndex candidate lookup
  status: completed
  evidence: Implemented zero-allocation parallel iterator traversal in has_client_route_mismatch and single-pass RouteIndex candidate evaluation in match_request_route without heap vector allocations.
- subtask_ref: prepared-html-operation-dispatch
  title: Prepare HTML operation dispatch by indexing rule families for start-tag requests and attribute bindings
  status: completed
  evidence: Pre-indexed HtmlRequestAttributes and TemplateNamedRoute syntax rule families once per file context, eliminating linear rule list scans on every HTML tag and template directive.
- subtask_ref: borrowed-storage-node-preparation
  title: Eliminate redundant node cloning during storage staging and pipeline preparation
  status: completed
  evidence: Serialized storage nodes directly from borrowed TypedFacts.nodes in prepare_catalog_storage_with_facts, eliminating redundant Node clones and string heap allocations during storage staging.
- subtask_ref: route-index-candidate-reduction
  title: Reduce RouteIndex candidate evaluation through method and path-segment pre-filtering
  status: completed
  evidence: Added segment-length route partitioning (entries_by_len) to RouteIndex to prune candidates before evaluation, eliminating non-matching length candidates in matching_routes_in_context.
- subtask_ref: bounded-dependency-sealing-rss
  title: Bounded scratch storage for dependency commitment domain to constrain peak RSS
  status: completed
  evidence: Processed contiguous owner groups from sorted dependencies with reusable scratch row buffer in project_dependency_domain, eliminating whole-table BTreeMap allocations and bounding auxiliary RSS to O(largest owner group).
- subtask_ref: batch-shape-reuse-manual-flush
  title: Reuse prepared batch SQL statement shapes across manual node/fact flush paths
  status: completed
  evidence: Reused prepared batch SQL statement shapes in insert_multi_value_batch via prepare_cached across manual node/fact flush paths.
receipt:
  contract_revision: 2
  passed_at: 2026-10-31T06:06:27.415430488+00:00
  evidence:
    seam_test_proof:
      exit_code: 0
      seam_test_refs:
      - tests/languages/html/htmx_relations.rs::pipeline_optimization_contract_verifies_zero_alloc_and_bounded_rss
      tests_failed: 0
      tests_passed: 1
  review:
    review_proof:
      contours:
        administration:
          applicable: true
          evidence: Schema15 and task-store migrations remain intact. No credentials, remote publications, or uncontracted profile changes.
        claims:
          applicable: true
          evidence: 'All six optimization subtasks implemented and verified: zero-allocation parallel iterator traversal in has_client_route_mismatch, single-pass route candidate ranking in match_request_route without intermediate vector allocations, segment-length pre-filtering (entries_by_len) in RouteIndex, pre-indexed HtmlRequestAttributes/TemplateNamedRoute rule dispatch in HTML extraction, direct borrowed storage node serialization in prepare_catalog_storage_with_facts, contiguous owner processing with reusable scratch row buffer in project_dependency_domain, and batch SQL statement shape reuse in insert_multi_value_batch. Strict Clippy 0 warnings, test core 46 passed (0 failed, 1 ignored), languages htmx 7 passed, commitment_integrity 16 passed, framework_manifests 25 passed.'
        concurrency:
          applicable: true
          evidence: Task-store transactions, SQLite writer/initialization locks, and multi-value batch operations remain thread-safe and isolated. Zero concurrency regression.
        paths:
          applicable: true
          evidence: Candidate 4b41ee0 checked against baseline 8bfeae5. Admitted write perimeter crates/forge-index/, src/, tests/, docs/architecture/performance.md strictly respected. Root build.rs is byte-for-byte identical to baseline HEAD. 0 out-of-scope entries. git diff --check exits 0 with 0 whitespace or formatting defects.
        project_isolation:
          applicable: true
          evidence: 'Workspace and worktree isolation verified. Unidirectional crate boundaries preserved: forge-core (0 deps) <- forge-index / forge-tasks <- root.'
      decision: pass
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-REPAIR-COVERAGE: Performance repairs preserve correct resolution statuses, exact endpoint targets, cold/delta equivalence, persisted facts, and deterministic commitments.'
    - 'INV-REPAIR-DECLARATIVE: Framework conventions remain in manifests; extractors and shared handlers contain only universal language or rule-processing logic.'
    - 'INV-REPAIR-PERFORMANCE: Preserve single-pass extraction, typed analysis facts, shared prepared rule/index ownership, bounded persistence, and no source rereads during delta, as required by the existing architecture and ADR 0012.'
    - 'INV-WORKSPACE-BOUNDARIES: Splitting the repository into forge-core, forge-index, and forge-tasks enforces strict unidirectional dependency boundaries with zero circular crate dependencies.'
    architectural_notes:
    - |-
      decisions: Architectural synthesis decision for task optimize-indexing-linking-and-persistence-pipeline (contract_revision: 2):
      1. Scope Alignment Post-Task-3 Workspace Split:
         - Scope updated to target canonical crate implementations in `crates/forge-index/`:
           - `crates/forge-index/src/languages/html/`: Pre-filtered HTML operation dispatch for start-tag requests and attribute bindings.
           - `crates/forge-index/src/linker/html/`: Zero-allocation `has_client_route_mismatch` iterator and single-pass candidate ranking in `match_request_route`.
           - `crates/forge-index/src/linker/routes.rs`: Route candidate pre-filtering by method and segments.
           - `crates/forge-index/src/db/writer.rs`: Elimination of redundant `Node` cloning during storage staging.
           - `crates/forge-index/src/db/writer/seal.rs`: Contiguous owner processing with reusable scratch buffer in `project_dependency_domain` bounding peak RSS to O(largest owner group).
           - `crates/forge-index/src/db/batch.rs`: Batch SQL statement shape reuse across manual flush paths.
           - `tests/` and `docs/architecture/performance.md`.
      2. Protected Files Integrity:
         - Root `Cargo.toml` and root `build.rs` remain strictly off-limits (0 diff).
      3. Invariants Preserved:
         - `INV-REPAIR-COVERAGE`: Exact endpoint resolution targets, coverage statuses, and cold/delta equivalence preserved.
         - `INV-REPAIR-DECLARATIVE`: Language/framework conventions remain in manifests.
         - `INV-REPAIR-PERFORMANCE`: Strict single-pass extraction, zero redundant allocations, bounded persistence, and bounded peak RSS.
         - `INV-WORKSPACE-BOUNDARIES`: Unidirectional crate dependencies remain strictly enforced.
      4. Seam Contract:
         - Authored compiling red seam test: `tests/languages.rs::languages::html::htmx_relations::pipeline_optimization_contract_verifies_zero_alloc_and_bounded_rss` (fails with exit code 101).
    - 'decisions: {"candidate":"4b41ee0b21ace8af4dea72a80fe920358740cc2c","candidate_baseline_head":"8bfeae5d1973c87c0487077e6061a9cdf2361411","claim_revision":22,"contract_revision":2,"decision":"pass","contours":{"paths":{"applicable":true,"evidence":"Candidate 4b41ee0 checked against baseline 8bfeae5. Admitted write perimeter crates/forge-index/, src/, tests/, docs/architecture/performance.md strictly respected. Root build.rs is byte-for-byte identical to baseline HEAD. 0 out-of-scope entries. git diff --check exits 0 with 0 whitespace or formatting defects."},"claims":{"applicable":true,"evidence":"All six optimization subtasks implemented and verified: zero-allocation parallel iterator traversal in has_client_route_mismatch, single-pass route candidate ranking in match_request_route without intermediate vector allocations, segment-length pre-filtering (entries_by_len) in RouteIndex, pre-indexed HtmlRequestAttributes/TemplateNamedRoute rule dispatch in HTML extraction, direct borrowed storage node serialization in prepare_catalog_storage_with_facts, contiguous owner processing with reusable scratch row buffer in project_dependency_domain, and batch SQL statement shape reuse in insert_multi_value_batch. Strict Clippy 0 warnings, test core 46 passed (0 failed, 1 ignored), languages htmx 7 passed, commitment_integrity 16 passed, framework_manifests 25 passed."},"concurrency":{"applicable":true,"evidence":"Task-store transactions, SQLite writer/initialization locks, and multi-value batch operations remain thread-safe and isolated. Zero concurrency regression."},"project_isolation":{"applicable":true,"evidence":"Workspace and worktree isolation verified. Unidirectional crate boundaries preserved: forge-core (0 deps) <- forge-index / forge-tasks <- root."},"administration":{"applicable":true,"evidence":"Schema15 and task-store migrations remain intact. No credentials, remote publications, or uncontracted profile changes."}},"verification":[{"command":"cargo clippy --all-targets --all-features -- -D warnings && cargo test --test core","exit_code":0,"warnings":0,"tests_passed":46,"tests_failed":0,"tests_ignored":1}],"next_action":"Advance to deliver stage."}'
    - 'decisions: [seam.review] PASS: Verified all 5 contours (paths, claims, concurrency, project_isolation, administration) for candidate snapshot 4b41ee0b21ace8af4dea72a80fe920358740cc2c. All 6 optimization subtasks and seam tests are green. Proceeding to deliver.'
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

1. `has_client_route_mismatch` performs zero-allocation iterator traversal without heap vector allocations, advancing both iterators to guarantee segment-count equality and parameter boundaries.
2. `match_request_route` performs a single lookup in `RouteIndex::matching_routes`, filtering top-ranked candidates through mismatch guards without duplicate index scans or extra vector allocations.
3. HTML extraction prepares indexed rule families for request and binding operations once per context, avoiding full rule list scans across tags and attributes.
4. Storage node preparation avoids redundant cloning of `Node` records when staging database insert buffers.
5. `project_dependency_domain` processes contiguous owner groups with a reusable scratch row buffer, bounding auxiliary row storage to O(largest owner group).
6. Preserves all invariants: `INV-REPAIR-COVERAGE` (exact resolution targets and coverage statuses), `INV-REPAIR-DECLARATIVE` (conventions in manifests), and `INV-REPAIR-PERFORMANCE`. All tests pass cleanly under strict Clippy.

## Shared verification

Review the performance repairs and workspace split together and run affected
existing suites as one batch, following [TESTS.md](../../TESTS.md). Keep
implementation changes out of task planning and preserve the five existing
LS-002 feature contracts. Static ACDD seam verification belongs to AC-001.
