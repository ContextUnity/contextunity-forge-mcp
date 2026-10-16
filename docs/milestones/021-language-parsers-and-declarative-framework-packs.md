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
2. Implement cross-layer framework relations: admit fact extractors in `src/engine/languages/python.rs`, `typescript/frameworks.rs`, and `html.rs` to wire Django `{% url 'name' %}` backend route resolution, CBV `template_name` (`renders_template`), and Alpine.js `Alpine.store('name', ...)` ↔ `$store.name.*` lookup.
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
target: "Implement two-phase semantic fact bus for Django {% url %}, CBV template_name, Alpine.store, template_attributes, Pinia, and GridViewSpec with measurable coverage proof"
proof_policy: seam-test-first
contract_revision: 6
scope:
  - src/db/writer.rs
  - src/db/delta.rs
  - src/engine/languages/mod.rs
  - src/engine/languages/dependency_registry.rs
  - src/engine/languages/manifests.rs
  - src/engine/languages/manifests/framework.rs
  - src/engine/languages/manifests/django.toml
  - src/engine/languages/manifests/gridviewspec.toml
  - src/engine/languages/python.rs
  - src/engine/languages/typescript/mod.rs
  - src/engine/languages/typescript/frameworks.rs
  - src/engine/languages/typescript/linker.rs
  - src/engine/languages/python/linker.rs
  - src/engine/languages/html.rs
  - src/engine/languages/html/javascript.rs
  - src/engine/languages/html/htmx.rs
  - src/engine/linker/framework_relations.rs
  - src/engine/linker/resolution.rs
  - src/engine/linker/routes.rs
  - src/engine/linker.rs
  - docs/reference/framework-manifests.md
  - docs/architecture/indexing.md
  - tests/languages/html/
  - tests/languages/manifests.rs
status: ready
subtasks:
  - subtask_ref: alpine-store-fact-extraction-and-linking
    title: "Emit named Alpine.store and Pinia defineStore registration identities in typescript/frameworks.rs, link to template $store.<name>.<member>, and support code-marker activation for x-data and hx-get"
    status: pending
  - subtask_ref: django-template-and-route-linking
    title: "Extract CBV template_name and [[template_attributes]] (renders_template, uses_model, spec) in python.rs, and link {% url 'name' %} in html.rs and htmx.rs to backend routes"
    status: pending
  - subtask_ref: component-and-controller-injection-linking
    title: "Link Vue provide/inject, NestJS constructor-injected members to fields, and $dispatch events to @event and HX-Trigger listeners"
    status: pending
  - subtask_ref: qualitative-coverage-proof
    title: "Verify and measure qualitative graph coverage gain: edge count increases, zero unresolved references for registered templates/stores, and verified round-trip to GridViewSpec"
    status: pending
```

#### Acceptance

1. `Alpine.store('cart', ...)` and Pinia `defineStore('cart', ...)` produce named store facts that resolve template `$store.cart.open()` references; template code-markers (`x-data`, `hx-get`) activate relevant rule processing even without package manifest declarations.
2. Django CBV `template_name = "..."` and declarative `[[template_attributes]]` create typed `renders_template`, `uses_model`, and `spec` edges to HTML templates and model definitions.
3. Django `{% url 'name' %}` references in templates resolve to backend route definitions across nested `include()` paths.
4. Vue `provide`/`inject`, NestJS constructor-injected properties, and `$dispatch` ↔ `@event`/`HX-Trigger` relationships produce resolved semantic edges.
5. Qualitative coverage gain is explicitly proven: test suites demonstrate increased resolved edge counts with zero regressions in baseline relations.
6. Persisted producer-to-linker integration tests pass in `tests/languages/html/`.

---

### task: compiled-rule-ir-and-three-tier-profiles

```yaml
task_ref: compiled-rule-ir-and-three-tier-profiles
target: "Compile framework manifests into typed RuleIR indexed by Tree-sitter kind_id, add rule coverage verification, and establish Three-Tier language profiles"
proof_policy: seam-test-first
contract_revision: 6
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
