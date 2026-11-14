---
title: "macOS Compatibility, Cross-Platform Boundaries, and Diagnostic Hardening"
doc_type: plan
purpose: >-
  Complete and prove canonical test coordinates, portable installer and reload
  behavior, SQLite generation safety, exact inventory classification, and
  actionable boundary diagnostics on Linux and macOS.
depends_on: []
---

# macOS Compatibility, Cross-Platform Boundaries, and Diagnostic Hardening

Implement the remaining gaps against [cross-platform architecture](../architecture/cross-platform.md), [SQLite concurrency](../architecture/concurrency.md), and ADRs [0002](../adr/0002-generational-sqlite-concurrency.md), [0005](../adr/0005-filesystem-root-guard-and-symlink-safety.md), [0010](../adr/0010-incremental-delta-transitive-dirtying.md), [0018](../adr/0018-layer-dependency-boundaries-and-core-leaf-isolation.md), and [0021](../adr/0021-workflow-plan-and-milestone-lifecycle-hierarchy.md). Follow [source ownership](../../src/AGENTS.md), [test placement](../../tests/AGENTS.md), [test harness rules](../architecture/test-harness.md), and [verification](../../TESTS.md).

**Verified starting boundary.** The live working tree already has Workspace canonical-root/path accessors; BSD socket blocking, timeouts, flush and write checks in InstallerHarness; stale-reader eviction before replacement identity sampling; query identity sampling before rollback; and describe_diff / matches_with_reason. Preserve these changes and prove their consumers. Remaining gaps include manually formatted expected URIs, discarded checkpoint result rows (including writer.rs), unchecked cleanup/publication results, rollback receipt drift, and raw filesystem probes in deletion classifiers. Reload discovery already uses OS cfg boundaries and one CLI grammar predicate. Keep its cohesive file unless a demonstrated responsibility boundary requires a separately admitted move.

**Execution ownership.** Each Scope is a write allowlist of existing repository files; Target describes the outcome. Dependencies reference exact step titles. Steps 1 → 3 → 4 → 5 → 6 → 7 → 8 → 9 serialize shared architecture, pipeline, cache, server, and installer ownership. Step 2 follows step 1 and may run alongside steps 3–9: its files are disjoint from theirs. Step 10 joins steps 2 and 9. Include documentation changes in their owning behavior step; the final step verifies alignment. Complete a producer before starting its dependent consumer. Reaudit scopes and dependency closure before adding a file or moving a module. Use existing owners instead of nonexistent http_fixture.rs, wal.rs, or src/cli/reload/ scopes.

**In-place snapshots.** Preserve all pre-existing tracked and untracked changes. The native plan runner executes steps sequentially and gate_snapshot captures repository-wide content, not just Scope. Use that serial execution for this plan. Step 2's disjoint allowlist describes dependency independence; it does not authorize overlapping native gate runs. Capture each owned file's starting content; quiesce all writers before snapshots and restores. Never restore a repository snapshot over another writer's work. The coordinator alone updates this plan's Status/Evidence after a step settles. Restore a rejected attempt only within its allowlist after checking for intervening edits; preserve baseline changes. Dependent steps consume accepted predecessor content. This plan does not authorize commits, remote publication, benchmarks, or code changes during authoring.

**Design limits.** Keep forge-core a leaf and preserve canonical-root and symlink checks. Reuse generation locks, Merkle verification, the scanner walk, and production URI encoding. Introduce no persistent path registry, receipt format, dependency, or generation counter. Keep Cargo.toml, member manifests, build.rs, and .forge/acdd/profile.yaml outside implementation scopes. Preserve filesystem-reported UTF-8 spelling as inventory/URI input, then percent-encode once; do not globally case-fold or NFC/NFD-normalize stored keys. Exact-entry evidence belongs to scanning/classification, not per-component directory scans inside canonical_root or checked_child. The planning add-on's ADR 0024 reference has no corresponding file in this checkout; the existing ADRs above govern this work.

**Proof and cleanup.** Record commands, nonzero selected-test counts, passed/failed/ignored counts, platform/filesystem characteristics, and the tested content binding. Add separate tests for distinct failure workflows; use table rows only for variations of one seam. Keep pure tests in memory and exercise real files/SQLite/sockets/processes for their physical boundaries. For each new contract, temporarily restore the unsafe operation named below, observe a runtime assertion failure, then remove that sensitivity mutation. Keep faults local and use child-specific environments rather than global environment changes. Cleanup guards own connections, generation locks, listeners, threads, children, observer descriptors, permission changes, and temporary files before workspace drop. Assert cleanup after explicit teardown on success and failure; Drop supplies panic-path fallback. Unsupported host behavior is reported explicitly, not hidden behind ignored tests or counted as native macOS proof.

## 1. Complete Canonical Test Coordinate Helpers

- **Target**: Extend existing Workspace accessors with one production-encoded expected URI helper while preserving uncanonicalized temporary input roots.
- **Scope**: ["tests/common/workspace.rs", "tests/common/workspace_contract_tests.rs", "docs/architecture/cross-platform.md"]
- **Depends On**: []
- **Status**: ready

Reuse [file_uri_for_path](../../src/core/format/discovery.rs) through its existing public crate path. Add Workspace::file_uri(relative, line) and a root-accepting counterpart only where linked/worktree callers need it. Apply Workspace::path's relative-input rejection, canonicalize the root once, append the relative coordinate, and invoke the production encoder with useful fixture failure context. Keep Workspace::new under the original env::temp_dir() spelling. Preserve child-symlink rejection at the production confinement boundary.

**Acceptance and proof**: Cover spaces, quotes, +, %, #, non-ASCII UTF-8, optional line anchors, and absolute/traversal rejection. Retain literal encoded-byte assertions for representative reserved characters alongside comparisons with the production encoder. Exercise actual database confinement through an uncanonicalized macOS temp alias and an escaping ancestor/leaf symlink; retain the existing core confinement tests. Update architecture section 1 with the real helper signature. Run `cargo test --test core workspace_contract_tests::` and `cargo test -p forge-core --lib fs::tests::`. Bypassing canonicalization/encoding must fail the corresponding alias/encoding assertion. Close readers and remove owned symlinks before workspace teardown; prove one workspace's drop removes its files without affecting another workspace.

## 2. Migrate Expected Coordinates Across MCP and ACDD Tests

- **Target**: Route filesystem-derived URI and server-root expectations through the canonical substrate while preserving literal input/rendering fixtures.
- **Scope**: ["tests/mcp/freshness/reference_responses.rs", "tests/mcp/freshness/manifest_freshness.rs", "tests/mcp/freshness/governance_responses.rs", "tests/mcp/query_ranking.rs", "tests/mcp/context/sibling_worktrees.rs", "tests/mcp/context/navigation_and_search.rs", "tests/mcp/context/worktree_claims.rs", "tests/acdd/mcp/milestones_and_handoff.rs", "tests/acdd/mcp/claims_and_leases.rs", "tests/acdd/mcp/worktrees_and_isolation.rs"]
- **Depends On**: ["Complete Canonical Test Coordinate Helpers"]
- **Status**: ready

Replace manual expected file URIs, manual percent escapes, and repeated canonical-root comparisons in these consumers. Include manifest continuation hints, linked/sibling roots, optional line numbers, and unresolved-but-confined coordinates. Read [ACDD test instructions](../../tests/acdd/AGENTS.md). Keep malicious /etc/passwd payloads, literal URI render fixtures, selector syntax, and docs-check input links literal: they are not filesystem-derived coordinate expectations.

**Acceptance and proof**: Audit remaining coordinate assertions across tests/. Every remaining manual filesystem-derived expectation requires migration within this allowlist or a scope/DAG amendment before writing another file. Record justified literal fixtures; zero text matches alone are insufficient. Run `cargo test --test mcp freshness::`, `cargo test --test mcp context::`, `cargo test --test mcp query_ranking::`, and `cargo test --test acdd mcp::`, checking each filter selects tests. A raw temp-root expectation must fail on the macOS alias case; missing escaping must fail the reserved-character case on either OS. Close MCP sessions before temporary worktree removal and use existing harness cleanup.

## 3. Prove BSD HTTP Delivery and POSIX Installer Failure Safety

- **Target**: Prove complete BSD-safe HTTP downloads and preservation of existing installation state on transport, checksum, or interruption failures.
- **Scope**: ["tests/mcp/installer.rs", "install.sh", "skills/install.sh", "docs/architecture/cross-platform.md"]
- **Depends On**: ["Complete Canonical Test Coordinate Helpers"]
- **Status**: ready

Keep the fixture local to InstallerHarness; extract shared substrate only with two real callers. Preserve accepted-stream blocking reset, read/write timeouts, write_all, and flush. Add a deterministic multi-MiB body with verified length/digest, truncated-response failure, and unexpected 200-response disconnect coverage. Report request path, transfer context, exit status, stdout/stderr. Permit only documented abandoned-404 broken-pipe/reset errors with diagnostics. Surface server-thread failures during explicit teardown without a second panic in Drop.

Exercise a child PATH containing shasum but no sha256sum, checksum mismatch/missing entry, HTTP failure, spaces/quotes in paths, and signal-triggered installer cleanup. Retain POSIX shell, portable mktemp, Darwin target mapping, and existing config/skills preservation. Change scripts only for demonstrated gaps; co-evolve architecture section 4.

**Acceptance and proof**: Run `cargo test --test mcp installer::`, `sh -n install.sh`, and `sh -n skills/install.sh`. Assert failure preserves the prior binary/configuration/skills and removes download plus binary/client staging files under fixture HOME/TMPDIR/project. Use bounded child deadlines with kill-and-wait fallback. Stop/join the HTTP thread, close listener/streams, and prove the port can be rebound after teardown. Removing the blocking reset/full-write check must break native BSD delivery; accepting truncation must break integrity/preservation assertions.

## 4. Enforce Checkpoint Results and Atomic Publication Cleanup

- **Target**: Give cold and delta writers one checked WAL checkpoint contract and explicit publication/cleanup outcomes.
- **Scope**: ["crates/forge-index/src/db/cache.rs", "crates/forge-index/src/db/delta.rs", "crates/forge-index/src/db/writer.rs", "src/engine/pipeline.rs", "tests/core/database_guard.rs", "docs/architecture/cross-platform.md"]
- **Depends On**: ["Prove BSD HTTP Delivery and POSIX Installer Failure Safety"]
- **Status**: ready

Place shared checkpoint handling in the existing database lifecycle owner [cache.rs](../../crates/forge-index/src/db/cache.rs). Reuse it from [persist_delta](../../crates/forge-index/src/db/delta.rs), cold writer population, and [atomic_build](../../src/engine/pipeline.rs). Inspect (busy, log, checkpointed) for TRUNCATE and PASSIVE, including SQLite's no-WAL sentinel result. Define which outcomes permit each caller to publish; successful SQL execution alone is insufficient. Preserve the bulk-ingestion journal-mode sequence and generation locks.

Finish writer close/checkpoint cleanup before sealing identity/receipts. Under exclusive generation ownership, distinguish absent sidecars from unexpected errors and never unlink WAL/SHM while live handles can use them. Handle stage removal and receipt publication errors explicitly. Pre-publication candidate failure preserves the old readable generation/receipt; a post-commit cache failure reports the committed result accurately rather than claiming rollback. Preserve output compatibility through existing diagnostic channels. Update architecture section 3.

**Acceptance and proof**: Add distinct database_guard tests for an active reader blocking checkpoint completion, successful retry after releasing it, injected cold-consumer failure using existing PipelineTestHooks, and cleanup obstruction. Assert no false verified receipt, no replacement on pre-publication failure, and no owned .forge-stage-* / candidate WAL/SHM / payloads.bin leftovers after successful cleanup. Obstructed cleanup reports its path and preserves unrelated files; remove the obstruction and prove cleanup succeeds. Run `cargo test --test core database_guard::`, `cargo test --test commitment_integrity`, and strict Clippy. Discarding checkpoint rows or ignoring stage cleanup must fail the relevant assertions. Release all transactions/connections/locks before final filesystem checks.

## 5. Preserve Verified Receipt Integrity Through Delta Rollback

- **Target**: Make rollback recovery deterministic across APFS metadata changes while keeping tampered generations untrusted.
- **Scope**: ["crates/forge-index/src/db/cache.rs", "crates/forge-index/src/db/delta.rs", "src/engine/pipeline.rs", "tests/mcp/freshness/incremental_freshness.rs", "docs/architecture/cross-platform.md"]
- **Depends On**: ["Enforce Checkpoint Results and Atomic Publication Cleanup"]
- **Status**: ready

Trace verified_receipt_recovery_survives_rollback_and_rejects_tampering through trigger abort, rollback, close, sidecar state, and receipt identity. Record describe_diff before/after to identify the actual mutation. Preserve a matching receipt. If legitimate rollback cleanup changes identity, verify the unchanged pre-delta Merkle seal under the exclusive generation lock and sample stable post-cleanup identity before replacing the receipt. Never copy a new identity into a receipt merely because the transaction returned an error. Failed revalidation/publication invalidates trust with diagnostics and requires normal full verification before read admission.

Split receipt recovery, rollback, and tampering into separately named workflows using Workspace and IndexFixture. Cover absent/corrupt receipts, wrong root/database, unsafe permissions, generation/sidecar drift, publication failure, and successful delta retry after removing the abort trigger. Keep root/permission/commitment checks fail closed. Update architecture sections 3 and 5.

**Acceptance and proof**: Run `cargo test --test mcp freshness::incremental_freshness::` locally and natively on macOS. Assert old graph/output root after abort, matching recovered receipt after successful revalidation, tamper rejection, and correct graph after retry. Restore permissions, drop the trigger, close readers/writers, and assert the declared quiescent sidecar state before workspace drop. Blind identity refresh or skipped tamper verification must fail. Do not require byte-identical APFS timestamps after legitimate SQLite rollback.

## 6. Complete Reader Admission Ordering and Failure Diagnostics

- **Target**: Preserve reader ordering and expose receipt, generation, and transaction failures through real MCP admission.
- **Scope**: ["crates/forge-index/src/db/cache.rs", "crates/forge-index/src/db/reader.rs", "src/mcp/server.rs", "tests/mcp/freshness/admission.rs", "tests/mcp/freshness/incremental_freshness.rs", "tests/core/database_guard.rs", "docs/architecture/cross-platform.md", "docs/architecture/concurrency.md"]
- **Depends On**: ["Preserve Verified Receipt Integrity Through Delta Rollback"]
- **Status**: ready

Retain stale-slot eviction before replacement-baseline sampling, eviction before rebuild/delta, and query post-check sampling under the shared generation lock and active transaction before ROLLBACK. Preserve the original failure context when rollback/close also fails; invalidate an unverifiable slot. Keep SQLite lifecycle ownership out of forge-core.

Use [matches_with_reason](../../crates/forge-index/src/db/cache.rs) where callers distinguish absent from invalid receipts. Keep normal cache misses recoverable by full commitment verification. Surface publication/invalidation errors and publication refusal with path/reason context; optional receipt caching must not become an unexplained valid-read failure. Retain existing signatures where practical and audit callers before changing cache APIs. Use describe_diff for generation mismatches and co-evolve concurrency plus cross-platform sections 3/5.

**Acceptance and proof**: Extend failed-query, external-delta, idle-reader rebuild, and active-reader fencing seams; add isolated invalid-receipt/publication diagnostic cases. Run `cargo test --test mcp freshness::admission::`, `cargo test --test mcp freshness::incremental_freshness::`, and `cargo test --test core database_guard::`. Assert the next read works after query failure, a real writer cannot pass the shared fence, and foreign/tampered indexes remain rejected without replacement. Use channels/barriers with bounded waits. Release blocked readers and join/drain writer threads on every outcome. Reordered eviction or post-rollback identity sampling must fail the corresponding native assertion.

## 7. Classify Case and Unicode Renames From Exact Inventory Evidence

- **Target**: Admit case/Unicode spelling changes correctly on each volume while preserving excluded-path and symlink rejection.
- **Scope**: ["crates/forge-index/src/scanner/mod.rs", "src/engine/pipeline.rs", "src/mcp/server.rs", "tests/incremental/scope.rs", "tests/mcp/freshness/incremental_freshness.rs", "docs/architecture/cross-platform.md"]
- **Depends On**: ["Complete Reader Admission Ordering and Failure Diagnostics"]
- **Status**: ready

Replace deletion classifiers in delta_inner and Server admission that treat exists() or successful symlink_metadata() as exact spelling evidence. Reuse exact previous/current scanner keys and actual directory-entry spelling; put shared classification in the existing scanner owner used by both callers. If excluded-entry evidence is needed, collect it within the existing bounded traversal, avoiding per-modified-path directory walks, per-component scans in core path resolution, or a persistent second inventory. Unverifiable boundary evidence fails closed with the affected path.

Preserve rejection of an explicitly modified path that still exists with exact spelling but is excluded from inventory, and deletion of a never-indexed path. Keep linked-root resolution, complete source-change checks, checked_child confinement, and scanner file/byte limits. Use existing volume probing for test classification; OS identity and equal inode numbers alone do not prove exact spelling. Preserve filesystem-reported UTF-8 keys and percent-encode once. NFC/NFD inputs follow actual volume spelling/distinctness; do not change persisted normalization or receipt formats. Document this explicit policy in architecture section 2.

**Acceptance and proof**: Add separate direct-delta and same-server cases for Foo.rs → foo.rs, parent-directory case changes, rename then delete, NFC/NFD names, reserved URI characters, and linked roots. Sensitive volumes prove distinct names coexist; insensitive volumes prove old keys disappear/new keys appear without forced rebuild. Include excluded existing paths, unlisted changes, and escaping/leaf symlinks as negative controls. Compare final graph/commitments with a cold build using existing parity primitives. Run `cargo test --test incremental scope::` and `cargo test --test mcp freshness::incremental_freshness::`. Restoring the raw probe must fail the native case-folding assertion. Drop readers and remove cold databases/sidecars and fixture aliases before workspace teardown.

## 8. Close macOS Reload Identity and Process Cleanup Coverage

- **Target**: Prove native reload identity parity through existing OS-isolated discovery and CLI grammar.
- **Scope**: ["src/cli/reload.rs", "tests/mcp/installer.rs", "tests/common/mcp_client.rs", "docs/architecture/cross-platform.md"]
- **Depends On**: ["Prove BSD HTTP Delivery and POSIX Installer Failure Safety", "Classify Case and Unicode Renames From Exact Inventory Evidence"]
- **Status**: ready

Keep Linux /proc and Darwin libproc/KERN_PROCARGS2 discovery behind existing cfg boundaries with the shared server_invocation grammar. Preserve Linux installed-path, deleted-marker, device/inode, and literal-suffix protections. Strengthen Darwin executable admission only for a demonstrated native gap; do not impose Linux /proc spelling rules on Darwin. Reject unreadable/ambiguous process identity rather than signaling guessed PIDs. Avoid a file move solely to shorten reload.rs.

Add native macOS hardlink-retained old-image and unlinked-image cases beside the existing default/explicit-server test. Assert replacement exec with stable PID and usable MCP stream, while foreign executables, non-server commands containing serve, and literal deleted-suffix collisions remain unaffected. Extend in-memory Darwin argv cases for malformed/truncated input and argc/environment separation. Update architecture section 4.

**Acceptance and proof**: Run `cargo test --lib cli::reload::tests::`, `cargo test --test mcp installer::`, and `cargo test --test mcp transport::`. Run the [documented macOS cross-Clippy lane](../../TESTS.md#cross-checking-macos-targets-from-linux) from Linux and obtain native runtime results separately. Keep subprocess use restricted to this process/transport boundary. Reuse StdioClient and correct cleanup in its scoped owner if necessary. Every child has kill-and-wait fallback, stdout/stderr readers drain or stop and join, observers/sockets close, and waits have deadlines. Exercise timeout/early-child-exit cleanup as well as success; assert no fixture child survives. Weakened identity checks or missing cleanup must fail the identity/teardown assertion.

## 9. Report Optional Debug Log Failures Without Breaking Requests

- **Target**: Make debug-log failures observable while preserving best-effort request behavior.
- **Scope**: ["src/core/debug_log.rs", "tests/core/debug_logging.rs", "docs/architecture/cross-platform.md"]
- **Depends On**: ["Close macOS Reload Identity and Process Cleanup Coverage"]
- **Status**: ready

Replace silent directory/open/serialization branches and discarded write results in log_cli/log_mcp with one local error-reporting path shared by both callers. Report operation, destination, and OS/serialization reason on stderr. Preserve signatures, log_command behavior, valid protocol stdout, and best-effort semantics. Avoid recursive logging and new diagnostics containing full request contents. Align architecture section 5; lifecycle correctness errors remain owned by steps 4–6 rather than a blanket replacement of all ignored results.

**Acceptance and proof**: Extend debug_logging.rs through actual CLI/MCP logging calls for successful JSONL writes and deterministic .forge/log-file obstructions, avoiding root-sensitive permission assumptions. Assert the original operation succeeds, stdout stays valid, and stderr identifies the failure. Run `cargo test --test core debug_logging::` and strict Clippy. Suppressing the diagnostic must fail its assertion. Remove obstructions, close log handles, and verify workspace removal on success and failure.

## 10. Reconcile Documentation and Verify Native Cross-Platform Gates

- **Target**: Bind completed changes to accurate architecture and repeatable Linux/macOS evidence.
- **Scope**: ["docs/architecture/cross-platform.md", "docs/architecture/README.md", "docs/architecture/concurrency.md", "TESTS.md", ".github/workflows/ci.yml", ".github/workflows/release.yml"]
- **Depends On**: ["Migrate Expected Coordinates Across MCP and ACDD Tests", "Report Optional Debug Log Failures Without Breaking Requests"]
- **Status**: ready

Resolve every INV-XPLAT source claim against final code/tests, including byte-preserving Unicode, checkpoint outcomes, rollback trust recovery, optional diagnostics, and cleanup. Confirm CI covers ubuntu-24.04/native macos-15 and release packaging covers both Darwin and both Linux architectures. Add only demonstrated missing checks. Preserve system TMPDIR aliases and avoid locale/temp overrides that mask boundary defects; record observed locale, temp spelling, and volume characteristics. Source defects return to their owning step; changed architecture decisions require owner admission.

Run `contextunity-forge-mcp plan show macos-compatibility-and-boundary-hardening`, `contextunity-forge-mcp docs check`, both shell syntax checks, and `git diff --check`. Run the required repository gate `cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets`, the separate `cargo test -p forge-tasks --lib` gate, and CI workspace commands `cargo clippy --workspace --all-targets --all-features -- -D warnings` / `cargo test --workspace --all-targets`. Reuse fresh equivalent results where content/command coverage matches rather than repeating redundant suites. Record passed/failed/ignored counts.

**Native and chaos acceptance**: Require Linux and native macOS suite evidence bound to final content. Case-folding cases execute on a proven insensitive volume; sensitive behavior executes on Linux or a native sensitive volume. Cross-compilation proves compilation only. Keep native results pending without a runner; local success alone does not complete this step. Reconcile the full failure matrix: busy checkpoint, rollback, publication/cleanup obstruction, malformed receipts, symlinks, truncated HTTP, installer interruption, process timeout/early exit, and debug-log failure. Confirm faults are removed and fixture-owned stages, cold databases, sidecars, sockets, threads, descriptors, and children are released while unrelated files/processes remain intact. Source changes invalidate affected downstream evidence. This plan does not authorize triggering publication jobs or pushing changes.

## 11. Consolidate Test Matrices, Remove Duplicates, and Optimize Suite Runtime

- **Target**: Eliminate redundant heavy `#[test]` clones across the macOS diff, move misplaced unit/transport tests to their owning modules, and remove subprocess/cold-build/sleep bottlenecks without dropping any assertion or contract edge case.
- **Scope**: ["AGENTS.md", "tests/AGENTS.md", "tests/mcp/freshness/incremental_freshness.rs", "tests/mcp/freshness/admission.rs", "tests/incremental/scope.rs", "tests/mcp/installer.rs", "tests/mcp/transport.rs", "tests/mcp/test_read.rs", "tests/core/database_guard.rs", "tests/core/debug_logging.rs", "tests/common/workspace_contract_tests.rs", "crates/forge-core/src/fs.rs", "crates/forge-core/src/config.rs", "crates/forge-index/src/scanner/mod.rs"]
- **Depends On**: ["Reconcile Documentation and Verify Native Cross-Platform Gates"]
- **Status**: ready

Consolidate homogeneous variations of the same contract into single table-driven `#[test]` (`for case in cases`) functions sharing immutable base state built once outside the loop, fold overlapping duplicates into their owning seam tests, move misplaced unit/transport tests to their owning modules, and apply runtime optimizations:
- Cache discovered tool paths (`TOOL_PATHS`) and static stub/8 MiB archive fixtures via `OnceLock` in `InstallerHarness`, replace `thread::sleep(1200ms)` with PID/pipe exit polling, merge redundant archive/download/server setups, and move `stdio_client_*` tests to `tests/mcp/transport.rs`.
- Consolidate the 7 `delta_rollback_*` tests into 1–2 table-driven tests sharing a single built base workspace/cold snapshot copied per case; collapse the 5 spelling/alias freshness tests into 2 matrices; share one `Workspace` across `admission.rs` receipt/query recovery cases.
- Consolidate `tests/incremental/scope.rs` outside-inventory deletions, exact-spelling/alias renames, and coexisting spellings into 3 table-driven matrices, subsume redundant `reserved_path_uri_is_encoded_once_after_delta` into `"a #%.rs"`, and move pure unit test `exact_spelling_does_not_infer_identity_from_hardlinks` into `crates/forge-index/src/scanner/mod.rs`.
- Consolidate `tests/core/database_guard.rs` sidecar/stage recovery matrices and fold absent custom `tasks_db` into `code_map_target_rejects_non_canonical_and_configured_tasks_db`; fold `test_read.rs` self-overwrite aliases into `forge_read_handles_path_aliases_and_rejects_unsafe_paths`; merge `workspace_contract_tests.rs` URI checks into 1 test; replace `debug_logging.rs` fake `#[test]` child + fragile `libtest` stdout parsing with real compiled binary / direct sink checks; add RAII `Drop` tempdir cleanup guards in `crates/forge-core/src/{fs.rs,config.rs}`.

**Acceptance and proof**: Keep 100% of contract assertions, cleanup checks, and leak checks intact while measurably reducing `#[test]` duplication and test binary runtime across `core`, `incremental`, `mcp`, `workspace --all-targets`, strict Linux/Darwin Clippy, `git diff --check`, and independent closure re-review.
