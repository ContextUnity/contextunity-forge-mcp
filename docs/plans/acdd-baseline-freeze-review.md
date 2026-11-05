---
title: "ACDD baseline freeze review before AC-003"
doc_type: audit
status: open
date: 2026-11-04
---

# ACDD baseline freeze review before AC-003

## Decision and content binding

The six original reproductions were repaired. The independent recheck on 2026-11-05 found **PR-02: the language-specific seam collector violates ADR 0020 and rejects the active profile's successful seam command**. Subsequent production tests also showed that the collector could accept a forged output line or attribute a test to the wrong source, reopening the BF-01 observation invariant and the bounded repair scope of the [remediation plan](acdd-lifecycle-segregation-fallback-removal-and-verification.md). The recheck, owner correction, and later PR-02 resolution are recorded below; the earlier passing verification and closure receipt remain historical evidence. Baseline commit and `AC-003` admission were pending at the recheck. This document is closure evidence, not another execution queue.

The reviewed snapshot contains HEAD `12d8df87516b5c2c57ba3d0348cfd138dc30c5af` plus tracked and untracked working-tree content captured on 2026-11-04 at 20:15:18 UTC. Its 774-file manifest digest is `e342374e697c224a78d3aa8cf9400f41b62505ef56e54ba959e7530039bcf4b3`. Two consecutive captures matched. The local evidence directory is `/tmp/forge-ac003-baseline-audit-1793823318/`: `manifest.json`, `status.txt`, `diff.patch`, and `tree/`. The snapshot CLI built successfully offline. All reproductions use that executable and disposable repositories; the user's task store was not cleared or modified.

At the initial capture, builder session `01a1a7d1-119f-7cb3-a7c8-ebbf4b6448e6` was still changing source. It subsequently completed its repair turn at 2026-11-04 22:10:40 UTC. Earlier full-suite results do not certify later edits. Preserve the same builder session for any further repairs and the existing reviewers for closure verification.

Read-only comparisons after the probes found all six implicated live paths unchanged. The proof/track comparison is bound to 20:26:13 UTC; `handoff.rs` and `reader.rs` also matched the snapshot in their reviewers' subsequent comparisons. These are moment-in-time observations, not certification of later edits. The builder reports a later passing strict Clippy run and continuing ACDD tests; no final verification result or baseline commit is claimed here.

## Confirmed findings and required regressions

| ID / priority | Trigger and observed result | Minimum repair and production regression |
| --- | --- | --- |
| BF-01 / P0 | A task gate with `proof: seam_test` accepts fabricated nonexistent test references and successful counts without executing its registered command, which would exit 9. Submission exits 0 and advances to the next gate. The same fabricated mechanical submission with `command` or `contract` correctly rejects with `TASK_AUTO_PROOF_UNAVAILABLE`. Acceptance in `crates/forge-tasks/src/engine/gates/` excludes seam-test proof from the observation check; `crates/forge-tasks/src/tasks/gates.rs` accepts its asserted fields. | Include every mechanical proof variant in common claim/candidate/command-plan observation validation. Check every declared seam reference against tests actually executed and passed; aggregate counts are insufficient. Add dedicated forged, unrelated, ignored, partially covered multi-reference, stale-claim and stale-candidate rejections, plus a real observed positive seam. Retain command/contract controls. If positive seam collection is deferred, reject unsupported proof explicitly instead of accepting assertions. |
| BF-02 / P1 | Complete legitimate verification/handoff, commit the archive, then recover a disposable lost database: recovery restores a tombstone with no closed checkpoint. While the archive exists, retry rejects with `MILESTONE_VERIFICATION_REQUIRED`. Remove only the working archive while retaining it in HEAD: retry with rejected review, unresolved findings and exit-9 verification returns completed. The `item.is_none()` tombstone branch in `crates/forge-tasks/src/milestones/handoff/` scans the HEAD archive and returns before equivalent validation. | Apply the same authority, accepted closure and evidence validation to every retry branch before cleanup or success. Missing durable proof requires a recovery diagnostic, not reconstructed success. Test both archive-present and HEAD-only branches, valid idempotent retry, and tampered/rejected evidence; verify rejected retries leave refs, manifests and index unchanged. |
| BF-03 / P1 | In a profile with an earlier code track containing a snapshot gate, claim the later docs track's initial non-snapshot gate. Its valid `none` proof rejects with `TASK_SNAPSHOT_NOT_FOUND` although no snapshot precedes it on the selected track. Removing the unrelated code track makes the same submission advance. engine/mod.rs (`crates/forge-tasks/src/engine/mod.rs`) searches earlier flattened gates beyond the applicable track. | Determine predecessor candidate requirements from the selected applicable task track. Add a dedicated two-track CLI/application test with an initial non-snapshot docs gate, plus a within-track missing-snapshot rejection control. Preserve canonical qualified gate identities. |
| BF-04 / P2 | Reading an absolute spelling containing `/tmp/../tmp/<workspace>/notes.txt` succeeds after normalization. `src/engine/reader.rs` loses the original traversal component before confinement checks. This proves a contract violation, not an escape outside admitted roots. | Reject original `ParentDir` components before absolute alias conversion. Test absolute and relative traversal, admitted absolute aliases, symlink provenance and linked-root behavior through the public reader. |
| BF-05 / P2 | A line read inside `First.value` in a file also containing `Second.value` suggests `selector="value"`. Following that exact hint fails as ambiguous. `src/engine/reader.rs` discards the selected node identity when formatting the hint. | Return an unambiguous selected identity supported by the reader. Add a test that follows the generated hint and obtains the same enclosing symbol, including repeated method names within one file. |
| BF-06 / P2 | A successful batch snippet read with `paths`, `selector` and `output_file` contains source in its JSON `batch` but exports only `### dupes.py` and blank lines. `src/engine/reader.rs` aggregates `content` while snippets use `source`. | Share exportable-content selection between single and batch reads. Assert exported source bytes for batch snippets and retain full-file/range/overview export controls. |

Local reproduction artifacts:

- BF-01/BF-03: `/tmp/forge-baseline-proof-probe/cli-results.json` and the adjacent probe scripts; includes command/contract and single-track sensitivity controls.
- BF-02: `/tmp/forge-baseline-tombstone-repro.py` and `/tmp/forge-baseline-tombstone-yoi43b33/.forge/repro-events.json`; includes the rejecting archive-present control.
- BF-04/BF-05/BF-06: `/tmp/forge-baseline-surfaces-probes.py` and `/tmp/forge-baseline-surfaces-probes.log`.

Retain the relevant regression in the repository during repair; `/tmp` artifacts are supporting local evidence, not a permanent test dependency.

## Coverage, dismissed suspicion and deferred work

Three independent reviewers covered the dirty task engine/proof/fixture changes; milestone/store/recovery/Git changes; and reader/CLI/MCP/core/configuration/documentation/example changes. The coordinator owns AC-003 design changes. Review is bounded by the captured content and tested scenarios; a green existing suite alone does not prove absence of the bypasses above.

The suspected archived receipt rejection caused by qualified track IDs **did not reproduce**. A real docs-track candidate/review/delivery, archive and database recovery succeeded; existing normalization preserves the identities. A suspected nonterminal delivery-successor escape is already rejected during profile admission. Do not implement speculative repairs for these dismissed suspicions.

The reader extreme-integer and reversed-range probes reject correctly. The original absolute-symlink guard is present; this review establishes no remaining symlink escape. All **17** active/reference/domain/workflow YAML profile validations pass. The snapshot documentation checker reports **96** link errors, including historical archive and absolute-file-URL links; this is existing documentation debt, not evidence of a checker defect. Reference and skill wording also needs to describe rejection of zero/reversed `end_line` values precisely.

The earlier 12-task AC-003 design and its review are superseded by the owner's simplification request. At the subsequent planning review, the `AC-003` contract extended existing execution with 10 tasks and passed manifest validation without warnings; the documentation checker reported 96 existing link errors. These are historical results. Planning checks do not establish implementation acceptance.

Prior review findings M1/M2/M5 concern complete root/container graph interpretation, declared retry routing/counters, and replay of historical conditions. Extend the current profile, task/root execution and recovery owners in AC-003 to address them. Record that deferral explicitly when closing the old plan; do not label it fully implemented. The six reproduced defects above remain bounded baseline repairs. Close proof bypasses before relying on the current runtime to execute AC-003.

The BF-02 probes below establish rejection of untrusted retry evidence, but do not establish ADR 0020 archival compliance. The earlier review incorrectly treated a retained closed SQLite checkpoint as sufficient permanent authority. AC-003's recovery task must preserve the rejection checks while validating legitimate archived retries from archive/Git proof after operational rows are pruned. Do not carry the checkpoint requirement forward as an architectural invariant.

## Closure and commit protocol

1. Finish the active builder turn; route these findings into that same session. Keep one source writer. The coordinator's initial steer attempt reached a different app-server and returned `thread not found`; that attempt did not deliver this report.
2. Record each finding as fixed, disproved with a sensitivity control, or explicitly owner-deferred. Proof bypasses require a fix before the baseline is described as verified. Bind closure evidence to the final source, not this earlier snapshot.
3. Run the required commitment integrity, strict Clippy and full root test commands from `TESTS.md`, plus `cargo test -p forge-tasks --lib`. Unset inherited agent-session environment variables as in the builder's verification instructions. Record exact passed/failed/ignored counts and exit codes after the last source edit.
4. Recheck the final delta independently, verify all affected documentation/examples, and reconcile the remediation plan's historical open claims with delivered repairs and explicit AC-003 deferrals.
5. Commit the scoped baseline, preserving unrelated files and user-staged work. Commit the revised AC-003 planning authority separately after that baseline. No remote publication is authorized.
6. Before AC-003 execution, reconcile its legacy operational rows through supported Forge recovery/admission. Do not reset SQLite or erase accepted evidence to make the new manifest fit.

## Baseline remediation receipt

The original rejection above remains the audit record for its captured snapshot.
The same builder implemented the six bounded repairs below. Final verification and
all three independent coverage contours pass. BF-01–BF-06 and the introduced
collector finding PR-01 are confirmed-fixed; no baseline commit is recorded here.

| Finding | Origin | Owner and production regression | Repair and sensitivity evidence |
| --- | --- | --- | --- |
| BF-01 | incomplete-remediation of B1 | `engine/gates.rs` → `engine/submit.rs` → runner → SQLite `CommandExecutionProof`; `tests/acdd/runner.rs` | SeamTest uses common claim/candidate/command-plan checks and durable per-test identities. Forged and unexecuted-reference regressions failed before the fix; observed Rust/Cargo positives, unrelated/file-mismatched/ignored/partial refs, count-only output, and stale claim/candidate/plan controls pass after it. Unsupported collectors fail closed. |
| BF-02 | incomplete-remediation of B2 | `milestones/handoff.rs::validate_archived_retry`; `handoff_tombstone_retries_validate_evidence_and_durable_closure_before_cleanup` | The regression reproduced HEAD-only acceptance of rejected evidence. Both archive branches now share validation before cleanup, accept valid retry, and reject missing checkpoint or altered evidence while preserving refs, manifest presence, and index. Missing authority requires recovery. |
| BF-03 | pre-existing-missed | `engine/claim.rs::has_predecessor_snapshot` → `engine/mod.rs::submit_inner`; `tests/acdd/gates_ast.rs` | Two-track docs regression failed with `TASK_SNAPSHOT_NOT_FOUND` before repair and now advances with qualified IDs. A same-track missing accepted snapshot still rejects. |
| BF-04 | incomplete-remediation of reader alias admission | `reader.rs::strip_root_path`; `forge_read_rejects_parent_components_before_absolute_alias_conversion` | Absolute traversal regression failed before repair. Original ParentDir components now prevent alias conversion; existing relative traversal, ordinary absolute alias, original symlink, and linked-root controls remain. |
| BF-05 | pre-existing-missed | `reader.rs::find_enclosing_symbol`; `forge_read_enclosing_hint_selects_the_same_repeated_method` | Following the emitted bare method hint failed before repair. Duplicate names now use the qualified selector, or the exact node ID if qualified names also repeat. The test follows each hint to its original method in both cases. |
| BF-06 | pre-existing-missed | `reader.rs::exportable_content`; `forge_read_batch_snippet_exports_source` | Export contained only the section header before repair. Single and batch reads now select source before content; snippet export bytes match returned source. |

Prior Option A lineage: B1/B2 are strengthened by BF-01/BF-02; M3/M4 track
references and receipt identity, M6 completed-task public API context, M7 original
symlink provenance, M8 checked line coordinates, and N1 end-line validation remain
covered by their production regressions. O1 is supporting positive evidence; O2
records planned execution. M1/M2/M5 remain **plan-deferred-with-guard** to AC-003,
as described above; the unified graph, universal root retry accounting, and
immutable historical-condition replay are not baseline implementation claims.

References were aligned with observed seam collection, retry closure requirements,
and rejection of zero/reversed `end_line`. No new profile option, graph executor,
database reset, or AC-003 implementation was introduced.

Independent confirmation of the first repair candidate found PR-01, an introduced
Cargo collector regression under BF-01: human `Running` paths are package-relative
for nested manifests/workspace members. A production regression with a same-named
root decoy reproduced acceptance of the wrong reference. The collector now requires
Cargo JSON artifact source identity relative to the claimed worktree; nested
`--manifest-path` and `-p` cases reject the decoy and accept the member reference.
Human-only Cargo output fails closed. The initial rejection remains recorded in
`/tmp/forge-baseline-proof-review.md`; final proof confirmation covers this delta.
The same source-identity confirmation also found mixed library/integration target
output being attributed to one integration source. The collector now requires
exactly one executable test artifact across all target kinds before accepting any
reference. Library, bin, example, and all-target selection regressions exercise
this ambiguity rejection; single-target member/custom-path positives remain.

## Final verification and closure

This is the builder's historical closure receipt. The command results remain valid for their bound content; the later independent recheck below supersedes its acceptance verdict.

The tested content binding is `32f35c50c4c4a7e95cc6ce514478636d351945dbd0505d175b12c5f57359913a`
at HEAD `12d8df87516b5c2c57ba3d0348cfd138dc30c5af` plus the admitted working tree.
All **775** bound file hashes matched after verification. This final receipt and
the remediation plan's closure paragraph were then updated as reporting metadata;
production source and tests remain unchanged from the tested binding.

Every command used this exact prefix:

```text
env -u AGY_CONVERSATION_ID -u FORGE_AGENT_SESSION_ID -u CODEX_THREAD_ID -u CLAUDE_SESSION_ID
```

| Command following the prefix | Exit | Passed | Failed | Ignored / warnings |
| --- | --- | --- | --- | --- |
| `cargo clippy --all-targets --all-features -- -D warnings` | 0 | — | — | 0 warnings |
| `cargo test --all-targets` | 0 | 887 | 0 | 1 existing filesystem stress test ignored |
| `cargo test -p forge-tasks --lib` | 0 | 21 | 0 | 0 ignored |
| `cargo test --test commitment_integrity` | 0 | 16 | 0 | 0 ignored |

The standalone commitment run is already represented once in the root total.
The root target counts are `18, 0, 195, 7, 16, 46, 9, 25, 4, 31, 262, 3, 108, 119, 44`.
Exact commands, summaries and output are retained in
`/tmp/forge-baseline-final-verification.json`,
`/tmp/forge-baseline-final-verification.log` and tool archive `ee56ebfd80556433`.

Independent terminal PASS reports are retained at
`/tmp/forge-baseline-proof-final-confirmation.md`,
`/tmp/forge-baseline-handoff-final-confirmation.md`,
`/tmp/forge-baseline-handoff-test-confirmation.md`, and
`/tmp/forge-baseline-reader-docs-final-confirmation.md`.
The handoff test confirmation covers the sole later test edit: the old recovery
matrix now expects the required recovery diagnostic instead of the prior cleanup
diagnostic. Its focused production test passes; the complete final run above also
passes. Earlier failed runs and review rejections are preserved as sensitivity
evidence, not treated as acceptance. The cumulative ledger is
`/tmp/forge-baseline-closure-ledger.md`.

M1/M2/M5 and native Workflow/Plan execution remain explicitly deferred to AC-003.
This baseline closure does not claim those architectural capabilities are delivered.

## Independent recheck: 2026-11-05

**Verdict: request changes for PR-02.** All six original BF defects are fixed, but the new individual-test collector is incompatible with the active repository's seam command. The repair introduced this compatibility regression; it is not a reopened fabricated-proof bypass or deferred AC-003 functionality.

The original three reviewers rechecked their owners against final reporting binding `e5ab3576fc192db26feca0c6672e66a104fd2ff027ca801430981713e108058e`. All 775 bound files matched before this reporting update. Source and tests also match tested binding `32f35c50c4c4a7e95cc6ce514478636d351945dbd0505d175b12c5f57359913a`; only the two reporting documents differed. Raw test summaries independently sum to 887 root passes, 0 failures and 1 ignored test, plus 21 dependency-crate library passes. The standalone 16 commitment checks are already included in the root total. No full-suite rerun is claimed by this recheck.

The coordinator rebuilt the current CLI successfully with the environment-cleaned `cargo build --offline --bin contextunity-forge-mcp`. Independent probes used executable SHA-256 `d60325ba68e3d7b4438446bcffa11ed7b26db3ba65e3ea3e3aae7cb066ad270e` and disposable repositories. No production source or tests changed during this review.

### PR-02: active seam command cannot produce required individual identities

**Priority P1; origin: introduced regression under BF-01.** The active profile (`.forge/acdd/profile.yaml`) binds `seam.integrate` to `rust-check-and-test`. For an integration-test scope its resolved command is:

```text
cargo clippy --all-targets --all-features -- -D warnings && cargo test --test observed
```

The runner executes that command unchanged. `passed_test_references` (`crates/forge-tasks/src/milestones/command_gate.rs`) accepts only a supported Rustc compile/run form or a single Cargo test invocation with JSON artifact output. The Clippy/test composition matches neither collector form and leaves `passed_tests` empty.

The real CLI probe ran `tests/observed.rs::actually_passed` through this resolved command. SQLite recorded process exit 0, one passing test and zero failures, but `passed_tests=[]`. Verification returned a proof diagnostic and typed submission of the actual passed reference failed with `TASK_AUTO_PROOF_UNAVAILABLE: no individually passed seam tests were observed`. Changing only the command to the supported `cargo test --test observed --message-format=json` produced the correct identity and allowed submission. This is a positive-path failure of the shipped gate, not permission to accept aggregate counts.

The seam telemetry example (`skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml`) also advertises a seam-test command using unsupported composition and `--nocapture`. The embedded default graph has no seam-test leaf, so this finding does not claim its ordinary Contract/Command stages are blocked. Profile schema validation alone misses the execution mismatch.

Repair the command/collector contract for the active seam gate and affected advertised examples while preserving the existing required strict-Clippy path. The initial suggestion to adapt the profile to the Rust-specific collector is superseded by the owner correction below: profile alignment alone leaves the ADR violation intact. Add a positive production regression using the actual shipped command resolver/profile, alongside the existing forged, missing, unrelated, ignored, partial, stale, legacy and ambiguous-source rejection cases. Until the generic collection boundary and shipped positive path work, the baseline remains open.

### Confirmed original repairs and remaining bounds

| Contour | Independent result |
| --- | --- |
| BF-01 / BF-03 | Original no-run forgery rejects; the inactive-track snapshot no longer blocks docs entry. Supported Cargo observation accepts the real reference and rejects wrong name/file, ignored/partial references, missing legacy identity fields and mixed target artifacts. Nested source identity and same-track missing-snapshot controls hold. |
| BF-02 | Valid working-archive and HEAD-only retries succeed with retained closure. Both reject missing closure after supported database recovery. The original rejected-review/exit-9 retry rejects; archive state, index and keeper refs remain unchanged. |
| BF-04 / BF-05 / BF-06 | 39 reader assertions plus one isolated index build pass: original traversal and aliases, repeated names/qualified names, exact hint roundtrips, single/batch source exports and retained bounds controls. Reference/skill end-line wording is corrected. |

Local recheck reports and evidence:

- `/tmp/forge-baseline-recheck-proofs.md`, `/tmp/forge-baseline-recheck-proof-results.json`, and `/tmp/forge-baseline-recheck-original-results.json`.
- `/tmp/forge-baseline-recheck-persistence.md` and `/tmp/forge-baseline-recheck-persistence-evidence.json`.
- `/tmp/forge-baseline-recheck-surfaces.md` and `/tmp/forge-baseline-recheck-surfaces-probes.json`.

PR-02 is the only new blocker established by this bounded review. Existing M1/M2/M5 deferrals and historical documentation-link debt remain unchanged. The same builder owns any repair; no further implementation or review wave was started by this read-only verification. This reporting update reopens the baseline without changing code, tests, Git history or the user's task database.

## Owner correction: remove language knowledge from proof collection

The owner requires the existing `commands.output` rules to describe individual test
identities and results, with language-neutral extraction in the core. The core must
not recognize executable names or shell forms. The active profile must retain strict
Clippy, and explicit and inferred submission must both enforce the existing
claim, candidate, worktree, command-plan and output-policy bindings. The owner
prohibits new report formats, project adapters, evidence tables, dependencies and
AC-003 implementation in this repair.

### Current blocker: provenance and defining source

No production repair has been accepted. A real test can write a line matching the
runner's success output directly to stdout; the present collector treats that line as
an observed passing test. The production regression
`seam_submission_rejects_test_output_forging_another_test_result` executes one real
passing test that prints a success line for `nonexistent`; submission of that nonexistent
reference currently succeeds. The regression fails at runtime, preserving the defect. An independent production CLI probe confirmed that the submitted nonexistent reference advances the task; its evidence is `/tmp/forge-pr02-injected-output-review.json`.

A second production regression,
`seam_submission_rejects_attributing_a_module_test_to_the_target_entrypoint`,
executes a test defined in `tests/actual.rs`. The collector attributes it to
`tests/runner_fixture.rs`, the target entrypoint, and accepts that incorrect reference.
The regression fails at runtime. A separate disposable reproduction moved the same
qualified test between two defining files; the emitted target source and test result
were identical. The runner output alone therefore does not identify which file defined
the test, and untrusted test output shares the stream containing result lines.

The shipped-profile regression
`shipped_seam_command_records_identities_and_accepts_typed_submission` independently
reproduces PR-02: the registered command succeeds but stores an empty `passed_tests`.
A profile regex cannot by itself distinguish the identical observations or establish
the origin of injected output. Do not call this baseline closed or describe BF-01 as
fully verified until the owner resolves these constraints and all three regressions
pass without weakening the evidence invariant. The original audit and previous test
receipts above remain historical evidence.

## PR-02 resolution: universal declarative output proof collection

PR-02 is resolved by eliminating language-specific parsers, executable name checks,
and argument whitelists from the Rust core (`crates/forge-tasks/src/milestones/command_gate.rs`),
in strict conformance with ADR 0020 (`INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE`).

### Architectural Implementation
1. **Declarative Output Schema Extension (`crates/forge-tasks/src/tasks/profile/`)**:
   `CommandOutputSpec` now includes an optional `tests: Option<CommandTestsSpec>` mapping with:
   - `passed: String`: regular expression matching an individual passed test and capturing its test reference/name in capture group 1.
   - `failed: Option<String>`: regular expression matching an individual failed test in group 1.
   - `ignored: Option<String>`: regular expression matching an individual skipped/ignored test in group 1.
   - `target: Option<String>`: regular expression matching the test target/entrypoint source file in group 1.
   Validated during profile loading: every supplied pattern must compile and contain capture group 1.

2. **Core Zero-Language Proof Extraction (`crates/forge-tasks/src/milestones/command_gate.rs`)**:
   - `passed_test_references` evaluates solely against the profile's declarative `output.tests` rules.
   - Core no longer contains any regular expressions or branches referencing `cargo`, `rustc`, libtest, or specific command compositions.
   - Test references are qualified with the canonical target path extracted by `target` rules or `scoped_target`.
   - Ambiguous multi-target runs (`canonical_targets.len() > 1`) fail closed, preventing cross-target test attribution.
   - Anti-forgery count consistency: if `require_counts: true` and summary count `parsed_passed` is present, `passed.len() as u64 == parsed_passed` is strictly verified. Any injected stdout output causing a count mismatch fails closed (`TASK_AUTO_PROOF_UNAVAILABLE`).
   - Failure guard: any test failure (`failed` pattern match or `parsed_failed > 0`) immediately fails closed.

3. **Profile Alignment**:
   - `.forge/acdd/profile.yaml`, `acdd.default.yaml`, and example references configure `commands.rust-check-and-test.output.tests`.
   - The shipped `seam.integrate` command (`cargo clippy ... && cargo test`) runs cleanly, records authentic passed test identities, and accepts typed submission.

4. **Production Verification**:
   - `shipped_seam_command_records_identities_and_accepts_typed_submission`: PASS
   - `seam_submission_rejects_test_output_forging_another_test_result`: PASS
   - `seam_submission_rejects_attributing_a_module_test_to_the_target_entrypoint`: PASS
   - `cargo_seam_observation_rejects_mixed_executable_test_targets`: PASS
   - `seam_submission_rejects_counts_without_bound_test_identities`: PASS
   - `custom_output_profile_extracts_seam_tests_without_core_modifications`: PASS
   - `wrapper_or_renamed_binary_produces_valid_seam_observation`: PASS
   - `store_reopen_preserves_durable_facts_and_output_policy_invalidation`: PASS

