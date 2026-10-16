---
id: m-language-parsers-and-declarative-framework-packs
title: Declarative framework packs, rule externalization, and 3-tier language parsers
doc_type: contract
status: active
started_at: 2026-10-16T08:05:00+00:00
depends_on:
- m-declarative-framework-packs-and-cold-build-pipeline
owners:
- src/engine/languages/
- src/engine/ast/
- src/engine/linker/
- docs/reference/
- docs/architecture/
- skills/contextunity-forge/references/
invariants:
- 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository (commerce-release-update) must not regress across runtime parser and framework extraction changes.'
- 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions with explicit collision precedence (project rules precede bundled rules) and support replace = true total substitution.'
- 'INV-RULE-EXTERNALIZATION: Class hierarchy receivers, route decorators, and route call extraction rules execute from declarative manifest definitions without hardcoded Rust match branching.'
- 'INV-PARSER-UNIFICATION: New language profiles adopt SyntaxGrammarSpec declarative syntax kind tables (~80 LOC) with shared SinglePassExtractor traversal, replacing ad-hoc recursive AST walks.'
- 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic for unchanged semantics and pass cargo test --all-targets; newly admitted semantic facts deterministically increment edge counts without altering unrelated domain digests.'
related_plans:
- docs/plans/forge-mcp-audit-and-architecture.md
- docs/plans/declarative-tree-sitter-queries-study.md
---

# Declarative framework packs, rule externalization, and 3-tier language parsers

## Outcome and purpose

Deliver comprehensive declarative language and framework extensibility for ContextUnity Forge MCP:
1. Externalize hardcoded framework rules from Rust code into TOML/YAML manifests: migrate base class receiver hierarchies (`django.db.models.Model`, `BaseCommand`, `HttpRequest`, `pydantic.BaseModel`, `GridViewSpec`) from `python/receivers.rs` into `[[class_hierarchy_receivers]]`, and route decorators/calls into `[[route_decorators]]` / `[[route_calls]]` covering Django (`path`), FastAPI (`@app.get`), and NestJS (`@Get`), wired into `InheritanceIndexes::build`.
2. Implement cross-layer framework relations by extending the first task's typed manifest contract and existing extraction/linking seams. Keep framework API names, activation markers, argument positions, member paths, and relation-key recipes in manifests; keep language binding and generic fact matching in Rust. Cover Django named URLs/CBV attributes, Alpine/Pinia stores, Vue injection, NestJS properties, and framework events without framework-specific Rust dispatch.
3. Establish 3-tier language architecture: declarative `SyntaxGrammarSpec` (~80 LOC) for rapid new language onboarding, and shared `SinglePassExtractor` with enter/exit `ScopeFrame` stack in `src/engine/ast/mod.rs` to replace redundant function body traversals.
4. Co-evolve architecture documentation in `docs/architecture/indexing.md` and reference documentation in `docs/reference/framework-manifests.md`.

---

## Tasks in this milestone

### task: rust-to-toml-rule-externalization

```yaml
task_ref: rust-to-toml-rule-externalization
target: Externalize hardcoded class receivers, route decorators, route calls, and dependency injection rules into declarative manifest sections and wire linker consumption
proof_policy: seam-test-first
contract_revision: 7
scope:
- src/db/writer.rs
- src/db/writer/memory_budget_tests.rs
- src/db/delta.rs
- src/engine/languages/manifests.rs
- src/engine/languages/manifests/framework.rs
- src/engine/languages/manifests/django.toml
- src/engine/languages/manifests/gridviewspec.toml
- src/engine/languages/manifests/fastapi.toml
- src/engine/languages/manifests/nestjs.toml
- src/engine/languages/manifests/pydantic.toml
- src/engine/languages/manifests/unittest.toml
- src/engine/languages/python/receivers.rs
- src/engine/languages/mod.rs
- src/engine/languages/dependency_registry.rs
- src/engine/languages/python.rs
- src/engine/languages/typescript/mod.rs
- src/engine/languages/html/htmx.rs
- src/engine/linker/indexing.rs
- src/engine/linker/resolution.rs
- src/engine/linker/semantic_context.rs
- src/engine/linker.rs
- src/engine/ast/mod.rs
- src/engine/ast/routes.rs
- src/engine/ast/route_includes.rs
- docs/reference/framework-manifests.md
- docs/architecture/indexing.md
- skills/contextunity-forge/references/
- skills/contextunity-forge/SKILL.md
- tests/languages/manifests.rs
- tests/languages/python/
status: completed
subtasks:
- subtask_ref: externalize-class-hierarchy-receivers
  title: Support [[class_hierarchy_receivers]] in FrameworkManifest and wire InheritanceIndexes::build in linker/indexing.rs for Django, TestCase hierarchies, and GridViewSpec
  status: completed
  evidence: 'Completed with the current source-verified GridViewSpec API: 28 declarative receiver rules cover the 21 actual named class identities (excluding GridViewAction, which is a union alias) plus GridViewActionBase and six concrete action dataclasses. Re-export aliases are exact grid_view_spec.types paths while providers remain canonical defining modules; no GridView class or methods were invented. The persisted seam loops all 28 rules, asserting one exact provider/member read is external and one unique unknown member remains unresolved. Sensitivity proof: temporarily removed only GridViewMeta and the corrected persisted seam failed specifically on GridViewMeta.subtitle (unresolved vs external); restored bundle SHA-256 cdfbb22b9c11bdd223ef1eff3194c6f7df42a398ea0e42205391bd975e2cc282. Corrected persisted seam passed (1), and registry seam passed (1), strictly parsing docs examples and checking 28 GridView rules. Verified actual source HEAD a2d037dff7cef99b820fbf212d21042bc5a90ca7.'
- subtask_ref: externalize-route-decorators-and-calls
  title: Support [[route_decorators]], [[route_calls]], and [[di_calls]] in FrameworkManifest, wire context propagation in python.rs and typescript/mod.rs, and extract routes for FastAPI (Depends, APIRouter prefix composition), NestJS, Django URL namespaces/include, and GridViewSpec
  status: completed
  evidence: 'Completed after consolidated R1–R7/R8 repairs and final R5/R7 closure. Persisted proofs cover exact-bare vs leading-dot callee matching, legacy qualified Django path/re_path behavior, dynamic APIRouter and Nest Controller prefixes, Django include namespaces/prefixes, exact Depends/InjectService destination path+qualname, and all 28 source-verified GridView receiver rules. R5: cold and delta composition match for child rename/change/delete, parent-only prefix/namespace edit with unchanged child, and nested includes; composed handles retain deepest child canonical lookup context and exact original source/line, parent decoy endpoints cannot retarget, old foreign-source references are removed/rebuilt, no include sentinel emits handles, and child deletion removes route+edge. A bounded mutual-include cycle fixture asserts the exact finite route set and targets and cold equivalence. R7: handler_argument=None emits a handlerless route with zero handles; configured Some(index) emits a route only when a valid positional handler exists from that index (including middleware scan), while missing/keyword-only/literal configured handlers emit no route; valid one/two middleware cases preserve exact target and handle-edge counts. Focused persisted seam passed (1). Strict Clippy passed; full `cargo test --test languages` passed (255), full `cargo test --test linker` passed (102).'
- subtask_ref: co-evolve-skill-references
  title: Update shared skills/contextunity-forge/references/ with verified examples for all new manifest sections
  status: completed
  evidence: 'Root-owned framework-manifest reference, indexing architecture page, SKILL.md, TOML/YAML declarative examples, and GridViewSpec TOML example are synced to the typed schema and current source-verified GridView API. Existing registry seam strictly parses both declarative examples and the full GridViewSpec example and asserts 28 hierarchy receiver rules; `cargo test --lib project_framework_packs_merge_replace_and_activate -- --nocapture` passed (1). Skill validator passed: `uv run --offline --with pyyaml python /home/oleksii/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/contextunity-forge`.'
receipt:
  contract_revision: 7
  passed_at: 2026-10-16T10:40:53.880056015+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test languages
      exit_code: 0
      tests_passed: 255
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Frozen snapshot96d92f67388c802b24909c16cd714247e0e3037b verified against admitted contract7 scope. Tracked scoped files match candidate; seven untracked scoped additions were independently hash-compared with candidate Git blobs, all exact. git diff --check exit0. Current bounded diff changes only routes, route include composition, resolver, the persisted production seam, and matching manifest documentation. R1 admission closed; protected milestone frontmatter unchanged.
        claims:
          applicable: true
          evidence: Consolidated bounded closure PASS blackboard246; attributable runtime packet245. All prior R1-R8 findings closed. R5 now retains deepest physical child lookup context/alias/provenance while handles originate at composed routes; R7 None emits handlerless route while Some missing/keyword/literal-invalid suppresses facts and valid/middleware retains exact handlers. Independent complete cargo test --test languages && cargo test --test linker exit0, languages255passed/0failed/0filtered73.66s and linker102passed/0failed/0filtered5.71s, archiveb45cd9dec8c7b594 lines260/367. No new full review or unadmitted requirements.
        concurrency:
          applicable: true
          evidence: Source remained frozen with independent reviewer21 distinct from builder20. Generated foreign references are removed and rebuilt canonically. Current binary production probes verified parent-only prefix/namespace/position change, child route/position update, and child deletion with immediate exact cold nodes/edges/errors/full resolution coverage including evidence:17/21/0/15 equal for parent and child,12/11/0/11 equal after delete. Stale route and deleted-source coverage counts0. Existing seam verifies finite cyclic include terminal set and cold graph+coverage; no source changes during review.
        project_isolation:
          applicable: true
          evidence: All task operations bound to exact forge-mcp T1, authorized reviewer alias, and dedicated milestone worktree. Runtime inputs/databases/script confined to /tmp. Actual candidate WT binary SHA25653135f7e9665e308b8b52215028b3ce363201f1bed6feea00dad06ddbe6c7ac9 and timestamp newer than repaired source verified; installed CLI only used task coordination. Workspace identity guard on original route facts prevents crossed include lookup. No external project/source writes, push, main integration or benchmarks.
        administration:
          applicable: true
          evidence: Claim21/contract7 uses exact retrieved template; reviewer-021-rust-toml-rules-oleksii is independent of builder-021-rust-toml-rules-oleksii. All required contours satisfied and full closure consolidated by root authorization246. Candidate immutable; no receipt/status manual edits, task/Git reset or duplicate commit. Forge auto_commit:true delivery will own atomic receipt and task commit. Runtime script SHA256e641f5f2acb02104fb931a57e25f3cf770ccb28162880ac0469f47988762e07c and exact commands/roots/exit counts retained in245.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository (commerce-release-update) must not regress across runtime parser and framework extraction changes.'
    - 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions with explicit collision precedence (project rules precede bundled rules) and support replace = true total substitution.'
    - 'INV-RULE-EXTERNALIZATION: Class hierarchy receivers, route decorators, and route call extraction rules execute from declarative manifest definitions without hardcoded Rust match branching.'
    - 'INV-PARSER-UNIFICATION: New language profiles adopt SyntaxGrammarSpec declarative syntax kind tables (~80 LOC) with shared SinglePassExtractor traversal, replacing ad-hoc recursive AST walks.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic for unchanged semantics and pass cargo test --all-targets; newly admitted semantic facts deterministically increment edge counts without altering unrelated domain digests.'
    architectural_notes:
    - 'decisions: T1 authority/scope reconciliation: task specs corrected actual module paths and admitted cold/delta registry plumbing plus repository SKILL.md. Protected milestone frontmatter remains unchanged; future task scope corrections preserve original acceptance and sequence. Root owns completed docs/examples, builder owns source/tests. Strong DI proof exposed injects references discarded by canonical resolver allowlist; exact resolution.rs extension accepted before repair, focused persistence seam red then green with exact injects path/target assertions (no relaxation). Skill validation passed using offline cached PyYAML via uv. Benchmarks remain unrun under repository explicit-approval policy.'
    - 'decisions: R4 package identity reconciliation: builder verified local /home/oleksii/ContextUnity/projects/grid-view-spec/src/grid_view_spec/types/spec.py (GridViewSpec dataclass fields id/title/meta/config/blocks/layout) and types/block_base.py (GridViewBlockBase fields id/type/title/extra/style/trusted_style/lazy), plus actual subclasses. Exact anchored class inventory across local projects excluding envs/worktrees found no GridView class; demo has none. Do not invent GridView provider/method identities. Implement package hierarchy acceptance through source-verified actual GridViewSpec/GridViewBlockBase field reads and retain legacy alias; root docs/examples now name these precise classes. User was asked optional async for another intended GridView import/repo; independent repair continues, naming limitation remains explicit. Any later actual import is new concrete authority to reconcile, not justification for fabricated rule.'
    - 'decisions: User supplied full GridViewSpec class inventory after candidate ff1217e. T1 will expand bundled class_hierarchy_receivers to all 22 named verified local classes and inherited members, exact canonical providers. Supplied member lists differ materially from current local source (Meta/Config/Layout/content), so current source is default factual API authority pending optional version clarification. No blanket all-unresolved guarantee: unknown members, shadowing and absent receiver proof must remain unresolved. Reviewer returns candidate to BUILD before any edits.'
    - 'decisions: Explicit user follow-up confirms «Використати перевірений локальний API». Current local grid-view-spec source is authoritative for fields/methods of all supplied class identities; inaccurate original member lists are superseded. No version ambiguity remains.'
    - 'decisions: R8 source authority pinned: grid-view-spec HEAD a2d037dff7cef99b820fbf212d21042bc5a90ca7, source tree clean. User explicitly confirmed local API. Named inventory is21 actual requested classes plus GridViewActionBase and six real variants (GridViewAction is union alias), total28 typed rules. Source-verified grid_view_spec.types re-exports may be alternative bases with defining canonical provider retained. Root reference+full28-rule TOML example updated; semantic values mirrored canonical bundle as saved. Builder captured real focused red exit101 before expansion; zero-test --exact run excluded. Known/unknown persisted receiver proof required; no invented unionclass/provider or blanket unresolved guarantee.'
    - 'decisions: Consolidated repair admission (239 + all3terminal): onlyR5incomplete andR7introducedregression, no newplan/scope. R5 fullpath writer cold/delta->route_includes composedrefs->canonical resolver->persisted exacthandles/coverage loses childimport origin. Preserve composedsourceID/line but lookup provenance deepestchild canonicalcontext, sameworkspace, no globalshortnameguess or sourceI/O. Proofmatrix coldanddelta exactprefixroute->actualimportedhandler; parentdecoy mustnotretarget; nestedcomposition mustretainoriginalcontext; childchange/delete no staleedge+bothcoldparityhelpers. R7 fullpath registrytypedrule->routecalls->persistedroute: handler_argument None emitsroute0handles; Some1/Some2 missing positional incl keyword-only emitnone; validpositionalindex andmiddleware keep1/1/2handles; invalidliteralhandler/missingpathnone. Existingproofstrengthen BEFORE production with validparsedfixtures, realred expectedtuple+routeabsence failures; intentional regressionmutations kill originlookup andNoneguard independently. Currentmapperfresh295files6036nodes20151edges; writer refresh/residualscan afteredits mandatory. AllotherR1-R8closuresretained. Required focus/strictClippy/full languages/full linker thenfrozenboundedverification onlysame2clusters. Terminalledger distinguishes1introduced/1incomplete (notdominant), repair staysactivewriteunion; no contract-per-finding or newfullreview. No sourceedits untilofficialrejectreturnsreadybuild.'
    - 'decisions: Consolidated bounded closure PASS for candidate96d92f67388c802b24909c16cd714247e0e3037b/rev7 afterBUILD20 and REVIEW21. All three existing reviewers PASS, no residual defect. R5 deepest canonical child context, exact imported/relative handler targets, decoy rejection, parent-only/child-only updates and deletion cleanup, finite nested/cycle composition closed. Intermediate coverage gap closed by attributable official current-candidate runtime packet245: immediate parent and child delta each exact cold nodes17/edges21/coverage15/errors0; deletion nodes12/edges11/coverage11/errors0, no stale routes or deleted coverage. R7 None handlerless vs Some missing/keyword/literal/path-negative and valid/middleware scan boundaries closed. Independent complete languages255/0 and linker102/0 archiveb45cd9dec8c7b594 exit0, writer strictClippy0/focused1/full255+102 green. Closed R1–R4/R6/R8 unchanged; expanded28 GridView source/mutation evidence retained. Root authorizes SAME reviewer session to submit review pass with all required contours then claim/submit deliver using returned task-bound evidence; auto_commit true, no duplicate manual commit, no worktree edits. No main landing/push/benchmarks authorized.'
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

1. `class_hierarchy_receivers` in manifest TOML dynamically register framework receiver methods (including GridViewSpec `GridView` and TestCase hierarchies) and are actively consumed by `InheritanceIndexes::build` without hardcoded Rust pattern matching.
2. `[[route_decorators]]`, `[[route_calls]]`, and `[[di_calls]]` in manifest TOML propagate from `DependencyRegistry` through `LanguageContext` to extract backend routes and DI parameters for Django (including `include("app.urls")` prefix concatenation), FastAPI (`Depends` and `APIRouter(prefix)`), NestJS, and custom frameworks.
3. `skills/contextunity-forge/references/` and `docs/reference/framework-manifests.md` provide matching, syntax-valid examples of all manifest sections.
4. `cargo test --test languages` passes with zero regression in route and receiver resolution.

---

### task: two-phase-semantic-fact-bus

```yaml
task_ref: two-phase-semantic-fact-bus
target: Extend the delivered manifest mechanism with declarative cross-layer semantic rules and a shared typed fact bus; require strictly increased verified coverage without losing correct baseline relations; remove bypasses, changes without proven benefit, and redundant tests
proof_policy: seam-test-first
contract_revision: 12
depends_on:
- rust-to-toml-rule-externalization
invariants:
- 'INV-SEMANTIC-RULE-DATA: Framework identities, imports/exports, callees, argument positions, member containers, template syntax/activation markers, header names, and semantic key recipes introduced by this task live in validated manifests. Production Rust contains no dispatch, fallback, whitelist, or special case keyed to Django, Alpine, Pinia, Vue APIs, NestJS, HTMX, or GridViewSpec for these relations. Language syntax and lexical binding remain Rust-owned.'
- 'INV-SINGLE-RULE-OWNER: DependencyRegistry resolves bundled/project manifests into one typed LanguageContext contract consumed by existing extractors and linkers. Keep shared fact identity, matching, persistence, and invalidation framework-independent; retain existing LanguageLinker ownership. Moving framework constants into another Rust file, private registry, string protocol, or second linker does not satisfy externalization.'
- 'INV-SEMANTIC-REPLACE: Project-first merge and replace = true govern every admitted semantic rule, including activation. Empty replacement emits no facts or edges attributable to that pack; package declarations, markers, caches, and hardcoded defaults never restore replaced rules. Independently enabled packs retain their own behavior.'
- 'INV-EXACT-SEMANTIC-BINDING: Emit a resolved edge only for a unique, statically proven target within the applicable project/framework and lexical binding scope. Shadowing, rebinding, unrelated same-spelling APIs, ambiguous providers, and unsupported dynamic values do not acquire exact semantic edges. Preserve unrelated baseline relations.'
- 'INV-PRECISION-AND-COVERAGE-GAIN: Require strictly increased source-verified semantic coverage and preserve every correct baseline relation. Omit doubtful new edges; never accept reduced correct coverage as the price of this task. Invented targets, heuristic exact promotion, and coverage-driven fallback are forbidden. Zero unverified or false semantic edges in the admitted proof corpus and positive verified coverage gain are joint release conditions.'
- 'INV-MANIFEST-ONLY-EXTENSION: Equivalent retained semantic rules for a previously unknown framework execute by adding TOML/YAML only, with the same compiled binary and no Rust branch or framework-name registration. Manifest changes and removal invalidate stale facts, edges, and resolution coverage.'
- 'INV-CONTROL-REPOSITORY-PROOF: Validate the retained semantic changes against the real commerce-release-update control repository and its configured linked workspaces as well as hermetic fixtures. Enumerate and source-verify every added or retargeted semantic edge in the affected families. An unavailable corpus, unverified target, unexplained relation change, or false exact coverage blocks acceptance; aggregate counts and sampling do not replace this check.'
- 'INV-MINIMAL-PROVEN-DIFF: Review every current task change against this contract. Remove framework bypasses, unused/speculative abstractions, duplicate extraction paths, unrelated edits, redundant tests, and tests that only mirror implementation. Retain each production change only with a named admitted claim, production-seam proof, and demonstrated coverage benefit or a necessary supporting role in that benefit; retain each test only with a distinct behavior/failure mode and sensitivity proof. Delete functional additions with no verified improvement. Code volume, test count, and raw edge-count gain do not establish benefit or correctness.'
scope:
- src/db/writer.rs
- src/db/delta.rs
- src/engine/languages/mod.rs
- src/engine/languages/dependency_registry.rs
- src/engine/languages/semantic.rs
- src/engine/languages/manifests.rs
- src/engine/languages/manifests/
- src/engine/languages/python.rs
- src/engine/languages/python/render_context.rs
- src/engine/languages/typescript/mod.rs
- src/engine/languages/typescript/frameworks.rs
- src/engine/languages/typescript/linker.rs
- src/engine/languages/typescript/value_flow.rs
- src/engine/languages/python/linker.rs
- src/engine/languages/html.rs
- src/engine/languages/html/javascript.rs
- src/engine/languages/html/semantic.rs
- src/engine/languages/html/htmx.rs
- src/engine/ast/mod.rs
- src/engine/languages/vue.rs
- src/engine/linker/framework_relations.rs
- src/engine/linker/resolution.rs
- src/engine/linker/routes.rs
- src/engine/linker.rs
- docs/reference/framework-manifests.md
- docs/architecture/indexing.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/
- tests/languages/html/
- tests/languages/manifests.rs
status: completed
subtasks:
- subtask_ref: alpine-store-fact-extraction-and-linking
  title: Implement retained manifest-driven provider/consumer relations through existing binding and composition owners
  status: completed
  evidence: 'Historical slug; current contract12 generic CallToFile only. Final candidate32865507d28a4d061a51c3c21fb5d4b55a758fd9 retains one registry-owned manifest operation, strict shared DTO identity, actual module provider, exact imported alias proof and unique actual HTML target composition. No store/event/member feature is claimed. Complete production seam54syntax cases and cold/delta/reopen/custom-pack lifecycle; current415tests/strictClippy and finalfc5e corpus+32occurrences/+29edges/no losses. /tmp/021-r12-build26-closure-ledger.md and /tmp/021-r12-build26-corpus-proof.json; final independent REVIEW27 terminal required before this completion is submitted. Independent REVIEW27 PASS accepted and submitted: /tmp/021-r12-build26-review-terminal.md and /tmp/021-r12-build26-review-submit-result.json; no required residual. Metadata reconciliation only; frozen candidate bytes unchanged.'
- subtask_ref: django-template-and-route-linking
  title: Reuse task-one attribute/template/route mechanisms where required by proven coverage improvement; preserve correct baseline relations
  status: completed
  evidence: 'Current contract12 preserves delivered T1 route/template mechanisms; new retained semantic operation is manifest-driven imported Python call argument to actual HTML module, with no new include/named-route extraction channel. Exact3955 indexed inputs/3993 captured sourceconfig preserved all314727 baseline occurrences/all279551 baseline edges/allcoverage, adding32 audited renders_template occurrences29edges. Current54-case exact binding negatives preserve unrelated relations; /tmp/021-r12-build26-corpus-proof.json and complete8comparison artifacts.  Independent REVIEW27 PASS accepted and submitted: /tmp/021-r12-build26-review-terminal.md and /tmp/021-r12-build26-review-submit-result.json; no required residual. Metadata reconciliation only; frozen candidate bytes unchanged.'
- subtask_ref: component-and-controller-injection-linking
  title: Remove unsupported semantic channels and retain only generic additions with exact binding proof and demonstrated benefit
  status: completed
  evidence: 'Current contract12 explicitly removed unsupported constructor/store/event/header/member/named-route/template_attributes additions. No unsupported channels reintroduced. Final current17builderfiles/1532added17removed mapped to minimal retained CallToFile operation and necessary identity/manifest/composition/invalidation support. Full20hash ownership inventory excludes root authority and2foreignformatfiles; /tmp/021-r12-build26-final-inventory.json and consolidated closure ledger.  Independent REVIEW27 PASS accepted and submitted: /tmp/021-r12-build26-review-terminal.md and /tmp/021-r12-build26-review-submit-result.json; no required residual. Metadata reconciliation only; frozen candidate bytes unchanged.'
- subtask_ref: qualitative-coverage-proof
  title: Prove strictly increased verified coverage with no lost correct baseline relations, exact targets, negative binding cases, custom-framework extension, merge/replace, and invalidation; source-audit added/retargeted edges on commerce-release-update and configured linked workspaces
  status: completed
  evidence: 'Current immutable fc5e322f003428dc6225e18898bd6ca545d916a4a947e33703c4ae6becced6bd CLI complete real-corpus proof:3955 indexed/3993 captured unchanged inputs,+32 individually source-audited occurrences/+29uniqueedges,zero baseline relation/coverage losses or retargets,zero false/unverified additions. Production reader complete32relations52participants,has_morefalse. Independent /tmp/021-r12-build26-proof-review.md evidence-closed; current54syntaxmatrix includes original28reviewcases and5compiled guard-removal runtime kills. /tmp/021-r12-build26-corpus-proof.json and closure ledger;  Independent REVIEW27 PASS accepted and submitted: /tmp/021-r12-build26-review-terminal.md and /tmp/021-r12-build26-review-submit-result.json; no required residual. Metadata reconciliation only; frozen candidate bytes unchanged.'
- subtask_ref: declarative-semantic-rule-contract
  title: Freeze the minimal typed semantic rules, identities, activation, merge/replace and invalidation contract through DependencyRegistry and LanguageContext
  status: completed
  evidence: 'Contract12 accepted actual Contract23; draft281 current bounded CallToFile, typed actual-module provider, shared identity/storage/invalidation. Scopeextension admitted existing canonical template_loaders helper for required precision repair with no contract expansion. Prior multi-channel drafts superseded. Unknown TOML/YAML/YML custom packs execute same binary; merge/replace/activation, namespace/project isolation and removal lifecycle proven in existing registered production seam. /tmp/021-r12-build26-closure-ledger.md;  Independent REVIEW27 PASS accepted and submitted: /tmp/021-r12-build26-review-terminal.md and /tmp/021-r12-build26-review-submit-result.json; no required residual. Metadata reconciliation only; frozen candidate bytes unchanged.'
- subtask_ref: minimal-proven-diff-cleanup
  title: Audit the entire existing task diff; delete bypasses, unsupported/unused code, duplicate paths, unrelated changes, and redundant tests; map retained changes to distinct production-seam claims and sensitivity proofs
  status: completed
  evidence: 'Complete current17builderfiles/1532added17removed necessary-role inventory against delivered T1 f45c764f5312aff1949231c967c59e66620362dd. Unsupported zero-gain channels removed, no parallel resolver/schema/testbinary; canonical helpers reused. All20 currenthashes bound,2pre-existingforeignformatfiles excluded and untouched. Complete54syntax equivalents, actualruntimeRed/5mutations/exactrestore/current415tests+Clippy; Forge source-map/residual receipts efb5da8e. Full /tmp/021-r12-build26-closure-ledger.md and finalinventory;  Independent REVIEW27 PASS accepted and submitted: /tmp/021-r12-build26-review-terminal.md and /tmp/021-r12-build26-review-submit-result.json; no required residual. Metadata reconciliation only; frozen candidate bytes unchanged.'
receipt:
  contract_revision: 12
  passed_at: 2026-10-16T19:39:11.682803965+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --lib --test languages --test linker
      exit_code: 0
      tests_passed: 415
      tests_failed: 0
      log: Complete consolidated closure /tmp/021-r12-build26-closure-ledger.md. Strict Clippy /tmp/021-r12-build26-clippy.log exit0; full tests /tmp/021-r12-build26-full-tests.log lib57+languages256+linker102=415passed/0failed/ignored/filtered.54-case production matrix; actual runtime Red;5 isolated compiled runtime guard-removal kills; exact restoration and Green. Final immutable CLI sha256 fc5e322f003428dc6225e18898bd6ca545d916a4a947e33703c4ae6becced6bd. Complete current corpus /tmp/021-r12-build26-corpus-proof.json:3955indexed/3993captured unchanged inputs,+29uniqueedges/+32auditedoccurrences,zero losses/retargets/false/unverified/coverage loss. Production reader32relations/52participants complete. All20 current inventory hashes verified;17builderfiles1532added/17removed; root authority/two foreign formatting files excluded delivery. /tmp/021-r12-build26-final-inventory.json. No source edits after final gates/capture. This command proof reports actual Cargo command receipts; evidence-export failures transparently retained in closure ledger.
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'PASS: complete prior perimeter retained, all four closure-path diffs/current matrix read, supported existing template_loaders.rs scope extension confirmed. Candidate32865507d28a4d061a51c3c21fb5d4b55a758fd9 all54 scoped blobs and20 inventory hashes current;17 builder files, root authority and two untouched foreign formatting paths separately owned/excluded. /tmp/021-r12-build26-review-currentness.json; complete terminal /tmp/021-r12-build26-review-terminal.md.'
        claims:
          applicable: true
          evidence: 'PASS: R12-C1 and R12-C2 closed through canonical argument/import-binding owner -> strict fact -> existing composer -> actual HTML persisted edge/coverage. Independent immutablefc5e CLI exact original28 replay:10 positives/18 declines, zero parser errors, facts/target/line/evidence/exact confidence/resolved coverage correct. /tmp/021-r12-build26-review-replay.json. Current54-case production matrix, five isolated compiled runtime guard-removal kills and exact restoration inspected; current root strictClippy0,57+256+102=415 tests pass. Complete current32occurrence/29edge corpus gain with zero loss/retargeted/false/relabelled baseline coverage accepted from fully read /tmp/021-r12-build26-proof-review.md. No required residual; full ledger /tmp/021-r12-build26-review-findings.json and terminal report.'
        concurrency:
          applicable: true
          evidence: 'PASS for diff: no claim/revision/receipt/rollback/lock owner changes; existing transaction and candidate fencing unchanged. Pure lexical proof reads introduce no global mutable state. Target maps complete before parallel immutable joins; delta owned replacement remains in existing transaction. No unexecuted competing-delivery result claimed. Complete terminal report.'
        project_isolation:
          applicable: true
          evidence: 'PASS: unchanged workspace-qualified nearest Python project identity and actual provider path/scope validation; canonical external-import proof, current nested-project/local-module/namespace controls retained green. Relative paths still reject absolute/traversal/invalid input. C1/C2 canonical proof repair adds no workspace/path authority bypass. Complete terminal report/current source binding.'
        administration:
          applicable: true
          evidence: 'PASS for REVIEW: exact finalfc5e CLI template binds reviewer claim27/contract12 and supported existing-helper scope extension. No schema/profile/gate/hook/protected receipt/header change; current scoped snapshot and actual HEAD T1 validated. No reviewer benchmark/commit/subtask/metadata/pass/delivery action. Scoped orphan snapshot is not claimed landed task commit; actual hook/parent/receipt/delivered SHA validation belongs to governed delivery, T3 HOLD. Complete terminal report.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-NO-COLD-REGRESSION: Cold build indexing throughput on control repository (commerce-release-update) must not regress across runtime parser and framework extraction changes.'
    - 'INV-FRAMEWORK-EXTEND: Project framework manifests under .forge/frameworks/*.{toml,yaml} extend bundled definitions with explicit collision precedence (project rules precede bundled rules) and support replace = true total substitution.'
    - 'INV-RULE-EXTERNALIZATION: Class hierarchy receivers, route decorators, and route call extraction rules execute from declarative manifest definitions without hardcoded Rust match branching.'
    - 'INV-PARSER-UNIFICATION: New language profiles adopt SyntaxGrammarSpec declarative syntax kind tables (~80 LOC) with shared SinglePassExtractor traversal, replacing ad-hoc recursive AST walks.'
    - 'INV-ZERO-SEMANTIC-REGRESSION: Existing symbol resolution, edge counts, Merkle commitments, and cross-file relationships remain deterministic for unchanged semantics and pass cargo test --all-targets; newly admitted semantic facts deterministically increment edge counts without altering unrelated domain digests.'
    - 'INV-SEMANTIC-RULE-DATA: Framework identities, imports/exports, callees, argument positions, member containers, template syntax/activation markers, header names, and semantic key recipes introduced by this task live in validated manifests. Production Rust contains no dispatch, fallback, whitelist, or special case keyed to Django, Alpine, Pinia, Vue APIs, NestJS, HTMX, or GridViewSpec for these relations. Language syntax and lexical binding remain Rust-owned.'
    - 'INV-SINGLE-RULE-OWNER: DependencyRegistry resolves bundled/project manifests into one typed LanguageContext contract consumed by existing extractors and linkers. Keep shared fact identity, matching, persistence, and invalidation framework-independent; retain existing LanguageLinker ownership. Moving framework constants into another Rust file, private registry, string protocol, or second linker does not satisfy externalization.'
    - 'INV-SEMANTIC-REPLACE: Project-first merge and replace = true govern every admitted semantic rule, including activation. Empty replacement emits no facts or edges attributable to that pack; package declarations, markers, caches, and hardcoded defaults never restore replaced rules. Independently enabled packs retain their own behavior.'
    - 'INV-EXACT-SEMANTIC-BINDING: Emit a resolved edge only for a unique, statically proven target within the applicable project/framework and lexical binding scope. Shadowing, rebinding, unrelated same-spelling APIs, ambiguous providers, and unsupported dynamic values do not acquire exact semantic edges. Preserve unrelated baseline relations.'
    - 'INV-PRECISION-AND-COVERAGE-GAIN: Require strictly increased source-verified semantic coverage and preserve every correct baseline relation. Omit doubtful new edges; never accept reduced correct coverage as the price of this task. Invented targets, heuristic exact promotion, and coverage-driven fallback are forbidden. Zero unverified or false semantic edges in the admitted proof corpus and positive verified coverage gain are joint release conditions.'
    - 'INV-MANIFEST-ONLY-EXTENSION: Equivalent retained semantic rules for a previously unknown framework execute by adding TOML/YAML only, with the same compiled binary and no Rust branch or framework-name registration. Manifest changes and removal invalidate stale facts, edges, and resolution coverage.'
    - 'INV-CONTROL-REPOSITORY-PROOF: Validate the retained semantic changes against the real commerce-release-update control repository and its configured linked workspaces as well as hermetic fixtures. Enumerate and source-verify every added or retargeted semantic edge in the affected families. An unavailable corpus, unverified target, unexplained relation change, or false exact coverage blocks acceptance; aggregate counts and sampling do not replace this check.'
    - 'INV-MINIMAL-PROVEN-DIFF: Review every current task change against this contract. Remove framework bypasses, unused/speculative abstractions, duplicate extraction paths, unrelated edits, redundant tests, and tests that only mirror implementation. Retain each production change only with a named admitted claim, production-seam proof, and demonstrated coverage benefit or a necessary supporting role in that benefit; retain each test only with a distinct behavior/failure mode and sensitivity proof. Delete functional additions with no verified improvement. Code volume, test count, and raw edge-count gain do not establish benefit or correctness.'
    architectural_notes:
    - 'decisions: Draft233 observable target refinement accepted by root: event dispatches oracle uses exact emitter path+line and listener path+unique attribute line/span, destination must not be module; distinct unrelated event/listener rejects name-blind joins. Alpine handler calls actual onEvent definition. Vue injects targets actual provided function. Nest class-owned injected field targets actual provider class and consumer member references bind to field; ordinary constructor parameter is negative. No listener-private schema frozen.'
    - 'decisions: Root owns docs/reference/framework-manifests.md and docs/architecture/indexing.md for T2; current draft covers frozen template_attributes schema and real cross-language graph/delta behavior, pending source/proof reconciliation before BUILD pass. A scope check for SKILL.md + declarative examples was rejected TASK_SCOPE_INVALID before any edits; T2 claim6/rev6 remains unchanged. Upcoming T3 author has explicit instruction to admit shared skill/examples in its contract revision before edits, alongside grammar/IR instructions, and root will reconcile the full milestone schema there. No changes outside current T2 scope. No benchmarks are authorized; throughput remains unmeasured under benchmarks/AGENTS.md.'
    - 'decisions: Bounded fixture authority correction, all acceptance assertions preserved: T2 explicitly admits CBV template_name, verified local Django .venv/lib/python3.13/site-packages/django/views/generic/base.py184–229 defines TemplateResponseMixin.template_name at187 and TemplateView inheritance at222. Bundled Django may supply import-proven hierarchy-restricted template_name/renders_template. model/spec on fixture CartView(TemplateView) are NOT verified built-in APIs; contract author confirms project RULES omitted their explicit declarations. Builder will add project template_attributes model→uses_model and spec→spec, each bases=[django.views.generic.TemplateView], leaving every edge assertion/CartView source intact. Existing entity/layout prove unrestricted rules. No invented GridViewSpec/TemplateView attributes in bundles, no contract/scope change or new harness.'
    - 'decisions: T2 continuation after user continue: existing BUILD claim6/rev6 retained under builder-021-semantic-facts-oleksii, new host writer task2_builder_resume owns same source/tests, root2docs only. Bounded oracle defect classified pre-existing-missed in contract test: two literal Django URL refs at template lines1/2 ARE emitted; src/db/ingest.rs618–675 writes separate edge_occurrences,697–712 aggregates canonical edges by(src,kind,dst) with occurrence_count. Test relation_rows33–39 reads aggregated edges, so original endpoint_edges.len()==2 contradicted persistence contract. Approved replacing ONLY faulty count oracle with one exact destination edge occurrence_count2 + exact two edge_occurrences lines1/2 + exact resolved coverage positions, preserving route namespace/composed handler/model/spec roundtrip and all delta assertions. No synthetic source nodes or global dedup/schema changes. Remove temporary instrumentation before candidate. Schema/coverage evidence linker393–401 +ingest749–760. Original missing-producer diagnosis superseded by actual trace.'
    - 'decisions: Second bounded contract-test oracle correction in same Django case: fixture Django path(''shop/''),path(''nested/''),path(''cart/'') is relative. Existing ast/routes.rs::prefixed_path retains no leading slash unless input prefix/path has one; ast/route_includes.rs128–132,180–209 composes those relative paths. Root approved selector expectation shop/nested/cart/ instead of /shop/nested/cart/, preserving exact outer:inner namespace, handler target/model/spec roundtrip, all occurrence/coverage and delta proof. Pre-existing-missed expected-value defect, not authority for changing production normalization or weakening runtime coverage.'
    - 'decisions: Third bounded pre-existing-missed Django oracle ownership correction, production unchanged: persisted root and middle composed route IDs DO each handles→actual py:views.py:14:cart_page, but Edge.path is leaf_urls.py, the physical handler occurrence. core/models.rs72–75 defines Edge.path occurrence ownership; route_includes.rs145–169,240–260 preserves deepest child source_path/source_line. Original relation_rows(urls.py,handles) filtered e.path_id (test33–39) and wrongly conflated route-node owner with occurrence owner. Approved querying leaf_urls.py for exact ROOT composed routeID→actual handler, with additional selected-root-node path assertion urls.py; retain namespace/path/occurrences/coverage/model/spec and all deltas, remove diagnostic dump. This preserves T1-delivered origin context rather than altering it.'
    - 'decisions: Explicit user steering: «це вже не смішно. ти 10 разів поспіль знаходиш щось нове. Дай агенту вказівку - використовувати скіл close-review-findings». Root acknowledges sequential symptom-fix process defect. Writer task2_builder_resume instructed to pause further production edits after currently running pre-instruction probe, read full close-review-findings +review-authority, refresh stable WT Forge architecture map and return ONE cumulative owner-cluster closure map/mutation matrix across ALL T2 producer/consumer families and cold/delta/persistence/ambiguity/scoping boundaries. Diagnose later scenarios independently instead of letting first panic hide them; preserve default complete accepted seam. Separate three known pre-existing-missed oracle assumptions from production hydration/closure defects with origin evidence. No new reviews/local patch cycles until root consolidates complete map, current map evidence and matrix; then one coherent repair+mutation+focused/full proof. Required map failures are blocking, text-search alone insufficient. Existing ownership/scope/claim6/rev6 unchanged; no new permission question needed for admitted BUILD work. No changes to receipts, main, benchmark authorization.'
    - 'decisions: User-directed close-review-findings consolidation accepted, builder task2_builder_resume resumes ONE coherent owner-cluster repair. Ordered map completed against current CLI Forge generation c595b5d5…a26bba3451 (296files/6089nodes/18370edges/0parseerrors) + AST/direct callers for known selfcrate map gap. Repair scope: canonical path_dictionary oracle join preserving route node owner + deepest-child occurrence/count/coverage; complete exact-path route include-family delta hydration; full accepted registry formats/modes/default/replace/collision/invalid matrix; existing integration seam independent family diagnostic aggregation and Vue/events/project isolation/framework-identity proof gaps. All helpers including Nest/events and every Django delta must execute, named mutations/residual/impact + strict Clippy/unfiltered languages/linker required. Every finding gets exact origin/disposition; include defect attribution requires actual snapshot evidence (not unsupported introduced-regression). No review/BUILD submit/claim/commit until consolidated evidence and root DOCS READY. Root owns only reference/framework-manifests.md + architecture/indexing.md, builder production/tests; no scope expansion or acceptance weakening.'
    - 'decisions: User explicitly directs continued comprehensive repair and rejects per-1/2-findings review cycles. Builder current aggregate executes all semantic families; remaining Nest field receiver proof diverges after provider restore, marker-free @click ordinary generic unresolved coverage is expected uncertainty. Root authorizes bounded oracle correction only after persisted evidence confirms generic call vs semantic-store-action distinction; preserve zero framework relation/coverage plus ordinary unresolved call. All family delta scenarios must execute to completion independently and complete consolidated packet required before review. No T2 completion claim. Root additionally auditing ownership only: newly dirty ast/mod.rs and tests/languages/support.rs require provenance/needed scope admission, ast/route_includes.rs and milestones/handoff.rs appear cargo-fmt-only; do not revert foreign changes.'
    - 'decisions: T2 minimal canonical-owner scope amendment: contract_revision6→7 admits only src/engine/languages/typescript/value_flow.rs, for existing named_type helper fn→pub(super) reuse by in-scope TS declaration typed FieldFact producer. Acceptance/proof policy/public interfaces unchanged; avoid divergent duplicate type parser or ad-hoc AST language hook. Official milestone CLI sync restarts contract; contract author reestablishes unchanged seam proof with explicit original pre-production Red provenance and fresh current scoped snapshot (omit commit override). Original Red source64b59ef… is provenance, not new candidate. Builder paused with no Cargo. ast/mod.rs, tests/languages/support.rs and confirmed cargo-fmt-only route_includes/handoff churn removed to cleanT1, no foreign reverts. Required node/edge/error+coverage parity and exact occurrences retained in admitted T2 seam. Build must re-claim updated rev7 after contract pass; root docs frozen READY (261).'
    - 'decisions: Root admits comprehensive BUILD packet /tmp/forge-021-t2-closure-ledger.md, all raw IDs K1-K4/R1-R2/A1/N1/E1/O1-O8/C1/P1-P2 one authoritative register, clusters K/R/A/N/E with runtime mutation kills and exact persisted cold/delta boundaries. Historical R restoration f8d08a… vs post-C1 final a1ab3… reconciled in ledger; no repo edit. Final strict Clippy0, unfiltered languages256/0 and linker102/0 after final syntax.rs exacttotalrefs2 assertion. DOCS READY261, docs metadata/12links/diff-check pass. Current CLI map e3f0a5…c467dd 296files6108nodes0parse, selfcrate caller gap explicit with live callers, no absence claim. Authorizes SAME writer BUILD pass only using exact claim9/rev7/template and auto-captured current candidate (omit commit override); no source edits/review/delivery. Review launches after returned immutable candidate on complete same scope, cumulative ledger, 3 independent dimensions, all5contours, full runtime targets.'
    - 'decisions: Review claim10 rejected successfully; returned build/ready claim10 contract7. Writer release authorized same worker. Canonical repair authority remains265 + /tmp/forge-021-t2-consolidated-repair.md + terminalreports. Correction to reviewer rejection HUMAN-READABLE path labels only: engine owners use src/engine/ prefix; N2 src/engine/languages/typescript/mod.rs:47, P3 tests/languages/html/framework_relations.rs:415/439/1247. Any rejection labels frameworks.rs forN2 or tests/integration/semantic_facts.rs forP3 are wrong and superseded; no such extra scope is authorized. Correctreports/exactruntimeevidence/4canonicalfindings unchanged. Buildsamewriter oneintegratedrepair, no furtherreview until completecumulativeadmission.'
    - 'decisions: P3 proof clarification within existing authority/scope (no contract amendment): tests/languages/support.rs::cold_parity applies a delta before comparison. Calling it after a multi-path move, even on moved-to path, cannot prove the required immediate transition. Builder now executes actual multi-path writer::delta -> assert_cold_equivalent (fresh cold DB, edges/full coverage) -> task-local read-only nodes/errors comparison using existing .forge/cold.sqlite + reader::open + commitments::verify, before ANY additional delta/edit. support.rs unchanged/out-of-scope. Three accepted moves must retain this immediate full comparison. Fresh authoritative full pre-fix aggregate compiled and reached all B1/L1/N2 independent families, exit101 at product-negative assertions; no P3 failure. Preserve earlier rejected-candidate origins and complete matrix; no new lifecycle/scope loop.'
    - 'decisions: Execution ownership handoff only, no contract/lifecycle amendment: root is now the sole source/test writer for the same admitted claim11/contract7 repair. Prior subagent interrupted and forbidden edits/Cargo/maps/submission; it confirmed no production edits had begun. Preserve its complete test matrix/latest corrected Red and ordered owner map. Root implemented initial cohesive actual-binding/literal/AST-modifier guards in admitted Python/TypeScript producers and exact provided-value composer, focused real persisted seam now running. Same independent reviewers and frozen scope remain. No new findings/capability/contract expansion.'
    - 'decisions: Root BUILD admission: cumulative ledger /tmp/forge-021-t2-closure-ledger.md supersedes earlier packet; ALL20 historical raw IDs plus current B1/L1/N2/P3 aliases/origins/dispositions preserved. Source frozen. New7 isolated guards compiled/runtime-killed/restored exactly; same-move nodes/errors+edges/fullcoverage executed. Final command receipts /tmp/forge-021-t2-root-gates/results.json: strictClippy0; full lib57/0, HTMX1/0, semantic1/0, full languages256/0, full linker102/0, diffcheck0, binarybuild0; source inventory checked after every gate. Current required WT CLI map ccb31e7f9405941c8e3a410bea2d823d745ffadbc21e3cd04ffd02b03c15b4dd,296files/6130nodes/18508graph edges/290docs/parse0; module impacts complete, known self-crate call gap retained and live callers verified. MCP AST retry still source-admission failure, preserved /tmp/forge-021-t2-root-map/ast-admission.json; native AST allowlist not bypassed. Consistent final binary64f2662b27c51727f486d83731dbd0599b5dc8f5a5704d2397b210599454542a. Root docs2headers/12links valid. Four existing subtasks evidence-completed. Ready to pass BUILD exact claim11/rev7, then same3 independent terminal dimensions ONE bounded full cumulative review. No source edits during review/deliver. T3 HOLD, no benchmark authorization.'
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

1. Extend the delivered task-one manifest mechanism before implementing further framework relations. Freeze the typed rule schema, canonical producer/consumer identity, activation and collision behavior in this task's Contract gate. Resolve missing choices there; keep framework externalization in this task. The successor compiles these admitted rules into RuleIR and supplies shared traversal; it does not own removal of this task's hardcode.
2. Trace every admitted rule through `.forge/frameworks/*` or a bundled pack -> strict manifest parsing -> `DependencyRegistry` -> typed `LanguageContext` -> existing language extractor -> shared fact matching -> persisted edges/coverage -> cold and incremental readers. Define each fact's owner, source occurrence, namespace, scope, and deletion/invalidation behavior. Keep framework-name protocols out of generic matching/storage logic.
3. Put package/import/export/callee identities, literal argument indexes, nested member paths, template token/attribute/header spellings, activation markers, and key projection rules in manifests. Keep AST traversal, lexical scopes, import/alias/rebinding analysis, TS constructor-property syntax, static-value extraction, and generic joins in Rust. Reject unsupported or malformed rule shapes with actionable diagnostics; never silently fall back to hardcoded framework semantics.
4. Extend task one with only the generic declarative operations needed for demonstrable coverage improvement on the unchanged control corpus. Framework identities and APIs are manifest data and examples, not a mandatory feature checklist. Reuse existing extraction, binding and linking owners. Preserve every correct baseline relation. Remove unsupported additions and additions without demonstrated benefit.
5. Prove manifest control on each retained rule family through persisted writer-to-reader tests. Start with one exact expected edge, apply an empty `replace = true` pack, and assert that its facts/edges disappear and its consumers receive the correct non-resolved coverage status. Keep package declarations and code markers present during replacement. Restore the rule and assert the exact target returns. Account separately for independently enabled packs.
6. Add a previously unknown project framework using TOML/YAML only and the same compiled binary. Exercise the retained provider/consumer mechanism with different API/import/export names, applicable argument positions, member paths, activation markers and key mappings. Assert exact destination path, qualified name, relation kind and occurrence. Rule retargeting must change the target; rule removal must remove its edges and exact coverage. A Rust whitelist or merely renaming a bundled pack fails acceptance.
7. Prove project-first collisions, additive extension, total replacement, strict TOML/YAML/YML parsing, and unsupported-field rejection. Change only a rule's API/member/key mapping and prove that graph behavior changes accordingly. Parse the shipped documentation/skill examples with the production schema.
8. Prove binding safety with same-name unrelated imports, aliases, lexical shadowing, reassignment before/after use, sibling/nested scopes, duplicate providers, project/package boundaries, dynamic keys, and missing targets. Cover every distinct binding mechanism admitted by this task. Assert absence of forbidden edges and absence of false exact coverage; assert the exact expected target in positive controls. Leave ambiguous or unsupported references non-resolved under the existing coverage policy.
9. Prove cold/incremental equivalence for producer/consumer edits and deletion, manifest edits/removal, changed activation, and changed namespace/key. Assert removal of stale edges/occurrences and stale exact coverage, plus unchanged unrelated baseline relations and deterministic commitments for equal final inputs. Include nested Django include paths and finite cycle handling when affected.
10. Audit the complete pre-existing task diff, including untracked tests. Remove code and tests that lack an admitted claim or useful proof, framework-specific bypasses, duplicate mechanisms, speculative abstractions, and unrelated changes. Consolidate repetitive fixtures without losing distinct binding/invalidation cases. Keep an evidence map of retained production changes and tests to claims and failure modes; identify deletions explicitly. Do not preserve code merely because it was already written or tests merely because they pass.
11. Use persisted production-seam tests in `tests/languages/html/` and focused manifest tests in `tests/languages/manifests.rs`; use unit tests only for a distinct validation boundary that the seam does not cover. For every retained test, name the defect/sensitivity mutation it detects. Demonstrate targeted failures with rule removal/retargeting and binding ambiguity, restore the candidate, then run the required checks. Positive edge-count increases, self-consistency assertions, and tests derived only from implementation are insufficient evidence.
12. Pass the affected test target and strict Clippy using the repository's required commands; preserve the milestone's existing verification gates. Review the final source diff for framework-name/API dispatch in Rust, including helper modules and database invalidation paths. Reject this task if any admitted relation still bypasses effective manifests or any tested ambiguous/unproven binding acquires an exact edge. Re-prove retained changes against contract revision 12; earlier completed subtasks or gate proofs do not satisfy these amended requirements.
13. Run a correctness comparison on the real `commerce-release-update` control repository and its configured linked workspaces. Record the resolved roots, Git revisions/dirty-input digests, effective manifest/configuration digests, candidate binary identity, and delivered task-one baseline identity. Keep source inputs identical between baseline and candidate; use separate disposable graph databases and leave the control repository unchanged. Read persisted edges/occurrences/coverage through the production reader. Enumerate all added, removed, and retargeted semantic relations in this task's affected families. For every added/retargeted edge, record source path/line, exact target path/qualified name, applicable rule, and source evidence proving scope, import/binding identity, and key compatibility. Source-check changed resolution statuses and disposition every removed relation. Sampling, aggregate coverage gains, and a baseline-only count are insufficient. Missing corpus access or an unverified change blocks acceptance. This is a correctness gate; performance benchmarking retains its separate explicit-approval requirement.
14. Require zero false or unverified semantic edges and zero false exact coverage in the hermetic and control-repository proof corpus. When a new target cannot be established, omit that proposed edge and keep the appropriate non-resolved coverage status; never manufacture a target to improve counts. This omission does not justify a coverage regression or a change without benefit. Convert each discovered binding defect into a focused distinct regression case, remove or correct its producing code, and repeat the affected corpus comparison before acceptance. Keep justified negative tests during cleanup. Report proof boundaries explicitly; passing the corpus does not establish correctness for all possible source programs.
15. Prove strictly positive verified coverage gain on the unchanged control corpus against the delivered task-one baseline, with zero lost correct baseline relations. Count source-verified resolved occurrences by source path/occurrence, relation kind, and exact target; keep the input/reference denominator fixed and report baseline, candidate, newly correct, lost correct, false, and unverified results by relation family. Raw edge multiplication, duplicate occurrences, or status relabeling do not count as gain. Prove each retained feature's improvement through a distinct positive production-seam case and identify its necessary supporting code; remove additions and their redundant tests when no improvement is demonstrated. Zero gain, reduced correct coverage, any lost correct baseline relation, or any false/unverified added target fails acceptance even if all tests pass.

---

### task: compiled-rule-ir-and-three-tier-profiles

```yaml
task_ref: compiled-rule-ir-and-three-tier-profiles
target: "Compile framework manifests into typed RuleIR indexed by Tree-sitter kind_id, add rule coverage verification, and establish Three-Tier language profiles"
proof_policy: seam-test-first
contract_revision: 7
scope:
  - src/db/writer.rs
  - src/db/delta.rs
  - src/engine/languages/mod.rs
  - src/engine/languages/manifests.rs
  - src/engine/languages/manifests/framework.rs
  - src/engine/languages/dependency_registry.rs
  - src/engine/ast/mod.rs
  - src/engine/ast/visitor.rs
  - src/engine/ast/routes.rs
  - src/engine/languages/python.rs
  - src/engine/languages/python/value_flow.rs
  - src/engine/languages/rust.rs
  - src/engine/languages/rust/value_flow.rs
  - src/engine/languages/typescript/mod.rs
  - src/engine/languages/typescript/value_flow.rs
  - src/engine/languages/vue.rs
  - src/engine/languages/vue/setup_facts.rs
  - src/engine/languages/vue/template.rs
  - docs/reference/framework-manifests.md
  - docs/architecture/modularity.md
  - docs/architecture/indexing.md
  - skills/contextunity-forge/SKILL.md
  - skills/contextunity-forge/references/frameworks/declarative-rules.toml.example
  - skills/contextunity-forge/references/frameworks/declarative-rules.yaml.example
  - tests/languages/
  - tests/core/
status: ready
subtasks:
  - subtask_ref: compile-framework-rules-to-kind-ir
    title: "Compile framework manifest rules into typed RuleIR dispatch tables indexed by Tree-sitter kind_id, re-compiling automatically on rule change"
    status: pending
  - subtask_ref: rule-coverage-verification-and-diagnostics
    title: "Implement rule coverage verification diagnostics warning or reporting if effective rule coverage degrades upon manifest substitution"
    status: pending
  - subtask_ref: syntax-grammar-spec-definition
    title: "Define SyntaxGrammarSpec struct for declaration, reference, scope, import, member-access, and comment kind mapping (~80 LOC)"
    status: pending
  - subtask_ref: single-pass-extractor-with-scope-frames
    title: "Implement shared SinglePassExtractor in ast/mod.rs and visitor.rs with enter/exit ScopeFrame stack, integrating Python, Rust, and TS binding and value-flow hooks"
    status: pending
```

#### Acceptance

1. Framework manifest rules compile into an in-memory `RuleIR` dispatch table keyed by Tree-sitter `kind_id`, avoiding nested TOML string matching on hot AST loops.
2. Changing a manifest triggers automatic re-compilation; rule verification diagnostics alert if symbol/route coverage drops on test suites.
3. `SyntaxGrammarSpec` enables concise language kind registration (~80 LOC) for declarations, references, scopes, imports, member access, and comments.
4. `SinglePassExtractor` traverses declaration bodies in a single DFS pass with `ScopeFrame` enter/exit lifecycle, integrating Python, Rust, and TypeScript binding and value-flow hooks without repeated subtree walks.
5. Existing language test suites pass with identical Merkle commitment digests for unmodified fixture sources.
6. `docs/architecture/modularity.md` is updated to formally document the Three-Tier language classification architecture (Tier 1: Declarative `SyntaxGrammarSpec` kind tables; Tier 2: Shared `SinglePassExtractor` AST traversal; Tier 3: Polyglot Semantic Linker). Declarative SCM queries remain an opt-in research direction governed by `docs/plans/declarative-tree-sitter-queries-study.md`.
