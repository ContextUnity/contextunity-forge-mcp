---
id: m-universal-multi-worktree-resolution
title: Universal Multi-Worktree Resolution and Stable MCP Roots
doc_type: contract
status: active
started_at: 2026-10-21T12:00:21.034253225+00:00
owners:
  - src/mcp/
  - src/engine/tasks/mod.rs
  - tests/mcp/
  - tests/acdd/
  - docs/reference/
  - skills/contextunity-forge/
invariants:
  - 'INV-STABLE-SERVER-ROOT: An omitted worktree always retains the canonical server root and configured index database, independent of active task claims, task settings, or claim transitions.'
  - 'INV-UNIVERSAL-WORKTREE-ROUTING: Every MCP tool uses the same explicit worktree resolver; task manifests and adapters come from the selected root while Git-linked worktrees retain their shared task store.'
  - 'INV-WORKTREE-MATCH-PRECEDENCE: Explicit paths and dot select directly; otherwise match exact name, prefix or zero-normalized milestone number, suffix, then substring. The first nonempty tier must have exactly one canonical candidate; missing and ambiguous selectors fail closed.'
  - 'INV-CANONICAL-CLAIM-AFFINITY: Task claim and next persist the canonical selected path, never an unresolved numeric or slug selector; submit retains existing claim, candidate, authority, and reviewer independence checks.'
related_plans:
  - docs/plans/universal-multi-worktree-resolution.md
---

# Universal Multi-Worktree Resolution and Stable MCP Roots

## Outcome and authority

Implement `docs/plans/universal-multi-worktree-resolution.md`, linked by `related_plans`.
The detailed implementation steps own matching precedence where the overview diagram differs.
The source plan's existing verification section is historical input, not proof of this candidate.

## Executable architecture

MCP request `worktree` → `Server::call_tool` → `resolve_target_root_and_db` →
`resolve_explicit_worktree` → request-local server/adapter/index → existing engine
operation → selected branch manifests and shared Git task SQLite → MCP result.

Without a selector, use the server root and its configured database. With a selector,
use the canonical selected root and its own default code index. Keep `.` explicit,
absolute and relative paths, registered external siblings, `.worktrees` candidates,
canonical deduplication, custom server database paths, and linked-server defaults.
Pure digit aliases compare milestone prefixes with leading zeros removed. Match
non-numeric prefixes, suffixes, and substrings literally. Reject empty, missing,
and ambiguous selectors without invoking the operation or mutating task state.

Route `task_list`, `task_blackboard`, `task_manage`, `task_claim`, and `task_submit`
identically to navigation tools. Keep required claim/next affinity semantics;
canonicalize their selectors before engine execution. A selected branch-only
manifest must be discoverable without copying it into main. Preserve task store
ownership, gate proofs, response budgets, strict unknown argument rejection,
configuration validation, and checkpoint isolation. Remove obsolete implicit-claim
routing helpers/state when no consumer remains. No storage migration, new tool,
implicit claim fallback, retry policy, or cross-repository behavior expansion.

## Proof and execution

Strengthen the existing `tests/mcp/context.rs` production stdio seams with
parameterized cases instead of duplicate standalone tests. Include `023`/`23`,
`060`, prefix, middle slug, suffix, exact precedence, ambiguity/missing/empty,
explicit paths, primary/linked stable defaults before and after claim transitions,
branch-only task sync/list/blackboard, canonical claim/next paths, submit, and
schema support. Adapt existing ACDD fixtures only where universal dispatch exposes
their assumed root/store relationship; retain their behavioral assertions.

Contract proof must compile and fail at runtime (101) against baseline routing.
Sensitivity: baseline implicit-claim selection or task routing bypass must fail
these same seams; disabling numeric/prefix/substring matching must fail selector
cases. Reuse the observed baseline red run where it proves sensitivity.

Build and review run the complete affected MCP and ACDD targets, focused contract
proof, and strict Clippy. A throwaway live MCP stdio scenario must observe selected
roots, branch-only manifests, canonical claim paths, and stable defaults. Remove
throwaway harnesses after recording evidence. Before handoff, independently scan
admitted docs, review the final combined content against main, and run the exact
compound gate in `TESTS.md` without filters. No benchmark execution.

## Tasks in this milestone

### task: universal-worktree-hub

```yaml
task_ref: universal-worktree-hub
target: Implement universal explicit MCP worktree routing, flexible selectors, stable defaults, canonical task affinity, and co-evolving documentation
proof_policy: seam-test-first
contract_revision: 2
scope:
- src/mcp/server.rs
- src/mcp/tools.rs
- src/mcp/tools/inputs.rs
- src/engine/tasks/mod.rs
- tests/mcp/context.rs
- tests/acdd/
- README.md
- docs/reference/mcp-tools.md
- docs/reference/tasks.md
- docs/reference/mcp-setup.md
- docs/reference/cli.md
- docs/runbooks/limits-and-freshness.md
- docs/plans/universal-multi-worktree-resolution.md
- skills/contextunity-forge/SKILL.md
- skills/contextunity-forge/references/
status: completed
receipt:
  contract_revision: 2
  passed_at: 2026-10-21T12:58:43.540870666+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test mcp --test acdd
      exit_code: 0
      tests_passed: 206
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Complete initial candidate review retained, with bounded4cc3d98..b688d3e5 repair independently reviewed: only two shared field docstrings and tasks.md routing table changed, all in admitted scope. No authority/runtime/type/storage/profile/config drift or reviewer edits. Candidate-scoped comparison across every effective scope path exited0.'
        claims:
          applicable: true
          evidence: 'Sole finding M056-R1 CONFIRMED-FIXED: mod.rs150-153,225-229 now documents Claim MCP aliases/canonicalization and Manage all-action routing/requirednext/parked affinity, explicitly distinguishing CLI/engine paths; unchanged JsonSchema flatten exposes corrected descriptions. tasks.md139-146,160-162 agrees. Parent pre-fix generated-schema smoke failed1, repaired all20 smoke passed0; same auxiliary reviewer and docs scan844 confirm closure. Four runtime invariants traced in initial complete source review retained. Independent required post-repair cargo test --test mcp && cargo test --test acdd exit0:90passed/0failed+116passed/0failed, no ignored/filtered; rawartifact://54. No remaining concrete findings.'
        concurrency:
          applicable: true
          evidence: 'Initial complete source review of Immediate transactions, claim/contract/worker/evidence-worktree fencing, candidate recapture, receipt locks and rollback retained: bounded repair changes documentation only. Fresh ACDD116/0 includes real competing MCP process next race, exactly one persisted claim, stale submissions and preserved parked-worktree affinity.'
        project_isolation:
          applicable: true
          evidence: Initial complete canonical-root/dedup and selected adapter/index/manifests/checkpoint isolation review retained; existing Git-shared task store, Registry namespace/scope and traversal/symlink/profile guards unchanged. Current graph inventory360 matched/0parseerrors; incomplete graph supplemented by exact source. Fresh MCP90/0+ACDD116/0 supports all retained production boundaries.
        administration:
          applicable: true
          evidence: No schema migration or privileged behavior change. Initial exact source review retained for pinned profiles, independent reviewer checks, immutable candidate/baseline recapture, hooked git commit, validated delivery tree/parent/landed SHA and atomic receipt rollback. Fresh ACDD116/0 includes tampering, hook rejection and candidate/receipt protections. Parent owns later milestone full compound gate; no milestone completion claim.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-STABLE-SERVER-ROOT: An omitted worktree always retains the canonical server root and configured index database, independent of active task claims, task settings, or claim transitions.'
    - 'INV-UNIVERSAL-WORKTREE-ROUTING: Every MCP tool uses the same explicit worktree resolver; task manifests and adapters come from the selected root while Git-linked worktrees retain their shared task store.'
    - 'INV-WORKTREE-MATCH-PRECEDENCE: Explicit paths and dot select directly; otherwise match exact name, prefix or zero-normalized milestone number, suffix, then substring. The first nonempty tier must have exactly one canonical candidate; missing and ambiguous selectors fail closed.'
    - 'INV-CANONICAL-CLAIM-AFFINITY: Task claim and next persist the canonical selected path, never an unresolved numeric or slug selector; submit retains existing claim, candidate, authority, and reviewer independence checks.'
    architectural_notes:
    - 'decisions: User superseding execution instruction: external Codex launch cancelled (bg_10); continue exclusively with native Forge MCP tools and built-in agents. Preserve interrupted work and accepted proof if any; inspect current gate before resuming. Repository Codex-launch recipe is superseded for this delivery. No external Codex sessions may be resumed.'
    - 'decisions: Pre-existing-missed test defect: global-skill content equality reads HOME and compares copied documentation; required repo skill co-evolution makes target fail despite all routing seams passing. Repository forbids external skill writes, and current engineering policy forbids incidental source/copy equality tests. Admitted tests/mcp/skill_sync.rs via extend_scope before deletion. Removed ONLY test_global_skill_matches_local_skill_when_present; preserved real hermetic installed-layout Markdown-link test and helpers. No skip/env override/global sync. Trace: docs co-evolution -> repo skill package -> existing user-level/repo-local temp install link validator -> MCP target. Full ACDD116/0 already observed; prior MCP90/1 includes only removed nonhermetic oracle. Also schema description defects routed same dispatch owner before candidate freezes.'
    - 'decisions: Native delivery proof: baseline runtime assertion failures101 for stdio numeric021 and indexed resolver049; final resolver unit1/0 and full registered strict Clippy&&MCP90/0+ACDD116/0 accepted build snapshot4cc3d98. Real final built-binary MCP stdio smoke (python /tmp/m056-native-smoke.py target/debug/contextunity-forge-mcp, exit0) observed7 aliases/all20 worktree schemas, branch-only manifest sync/list/board exact payload, canonical claim+next persisted paths, selected-root submit, stable default with active claims, checkpoint isolation, missing+ambiguous rejection. Hermetic Git fixture self-cleaned; throwaway harness removed after proof. Final documentation scan836/837 and milestone mirror835 reconcile actual source/schema/skill and archive-stable plan relation. Source-plan archive target is intentionally materialized by handoff and must resolve before final delivery. No external skill writes, skipped tests, benchmarks, or user-primary code inclusion.'
    - 'decisions: M056-R1 consolidated repair: changed ONLY Claim.worktree and Manage.worktree shared field docstrings in engine/tasks/mod.rs plus tasks.md action table clarification; MCP aliases/all-action routing now explicit, CLI/engine remain path-only/next-only, required next selector and parked retained affinity preserved. Pre-fix actual tools/list smoke exited1 at task_claim path-only description; post-fix actual tools/list smoke exited0, inspecting all20 worktree properties grouped18common+claim+manage. Strict registered cargo clippy --all-targets --all-features -- -D warnings && cargo test --test mcp --test acdd exited0, MCP90/0+ACDD116/0=206, rawartifact://47. git diff --check0; graph inventory360 matched/0parseerrors output_root0a62cebd. No runtime/type/storage behavior change, new schema adapter, permanent source-text test, skipped target or external install. Same independent reviewers must close R1 on repaired candidate before delivery.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
subtasks:
- subtask_ref: hermetic-skill-verification
  title: Remove environment-dependent installed-vs-repository content equality test while retaining isolated installed-layout link coverage; cargo test --test mcp must pass without external skill writes
  status: completed
  evidence: 'After bounded R1 inherited-schema/client-doc repair, final exact registered cargo clippy --all-targets --all-features -- -D warnings && cargo test --test mcp --test acdd exited0 again: MCP90/0, ACDD116/0; total206. Rawartifact://47. Retained real temp-installed-layout link proof; no external skill writes/skips/env overrides.'
```

## Deferred and out-of-scope defects

```yaml
deferred_defects: []
```
